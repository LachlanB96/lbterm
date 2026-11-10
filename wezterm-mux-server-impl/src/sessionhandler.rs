use crate::PKI;
use anyhow::{anyhow, Context};
use codec::*;
use config::TermConfig;
use mux::client::ClientId;
use mux::domain::SplitSource;
use mux::pane::{Pane, PaneId};
use mux::renderable::{RenderableDimensions, StableCursorPosition};
use mux::tab::TabId;
use mux::{Mux, MuxNotification};
use promise::spawn::spawn_into_main_thread;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use termwiz::surface::SequenceNo;
use url::Url;
use wezterm_term::color::ColorPalette;
use wezterm_term::terminal::Alert;
use wezterm_term::Progress;
use wezterm_term::StableRowIndex;

#[derive(Clone)]
pub struct PduSender {
    func: Arc<dyn Fn(DecodedPdu) -> anyhow::Result<()> + Send + Sync>,
}

impl PduSender {
    pub fn send(&self, pdu: DecodedPdu) -> anyhow::Result<()> {
        (self.func)(pdu)
    }

    pub fn new<T>(f: T) -> Self
    where
        T: Fn(DecodedPdu) -> anyhow::Result<()> + Send + Sync + 'static,
    {
        Self { func: Arc::new(f) }
    }
}

#[derive(Default, Debug)]
pub(crate) struct PerPane {
    cursor_position: StableCursorPosition,
    title: String,
    working_dir: Option<Url>,
    dimensions: RenderableDimensions,
    mouse_grabbed: bool,
    alt_screen_active: bool,
    sent_initial_palette: bool,
    /// LBTerm: the palette this client was last sent for the pane: one the same is never sent
    /// again (maybe_push_pane_changes)
    sent_palette: Option<ColorPalette>,
    /// LBTerm: whether this client has been sent what the pane already had
    /// before it attached
    sent_initial_state: bool,
    seqno: SequenceNo,
    config_generation: usize,
    pub(crate) notifications: Vec<Alert>,
    /// LBTerm: the seqno this client was told the pane's lines are held at (codec's PaneHeld),
    /// until a push carries them: that push goes whatever it finds, so the client knows its copy
    /// is current again (lbterm/MUX.md, "Tabs out of view: output held at the server")
    lbterm_held: Option<SequenceNo>,
    /// LBTerm: what the connection owes its client, told when a push carries the pane's lines
    /// (lbterm_flow::Pending::current), whichever of the connection's paths sent it
    lbterm_pending: Option<Arc<Mutex<crate::lbterm_flow::Pending>>>,
}

impl PerPane {
    fn compute_changes(
        &mut self,
        pane: &Arc<dyn Pane>,
        force_with_input_serial: Option<InputSerial>,
    ) -> Option<GetPaneRenderChangesResponse> {
        // LBTerm: read in one go, a local pane's terminal under one lock with the rows changed
        // since the last push, and its cwd, when its shell says none, as last listed
        // (mux/src/lbterm_push_read.rs); upstream asked a getter at a time, each locking the
        // terminal, and read the foreground's cwd there and then
        let read = pane.lbterm_read_for_push(self.seqno);
        crate::lbterm_pushstats::mark(crate::lbterm_pushstats::Step::Reads);
        let mux::lbterm_push_read::PushRead {
            mouse_grabbed,
            alt_screen_active,
            dimensions: dims,
            cursor_position,
            title,
            working_dir,
            seqno,
            changed: mut all_dirty_lines,
        } = read;
        let changed = mouse_grabbed != self.mouse_grabbed
            || alt_screen_active != self.alt_screen_active
            || dims != self.dimensions
            || cursor_position != self.cursor_position
            || title != self.title
            || working_dir != self.working_dir
            || !all_dirty_lines.is_empty();
        self.seqno = seqno;
        // LBTerm: the client has the pane's lines as of this read, by this push or the last, so a
        // hold of them ends, whichever path this is (a round's, a key's answer, a poll's); and a
        // hold the client was told of has this push sent whatever it finds, its seqno past the
        // one the client was told, which the pane had printed past (lbterm_out_of_view)
        let told = self.lbterm_held.take().is_some();
        if let Some(pending) = &self.lbterm_pending {
            pending.lock().unwrap().current(pane.pane_id());
        }
        crate::lbterm_pushstats::mark(crate::lbterm_pushstats::Step::Changed);

        if !changed && !force_with_input_serial.is_some() && !told {
            return None;
        }

        // Figure out what we're going to send as dirty lines vs bonus lines
        let viewport_range =
            dims.physical_top..dims.physical_top + dims.viewport_rows as StableRowIndex;

        // Always send the cursor's row, as that tends to the busiest and we don't
        // have a sequencing concept for our idea of the remote state.
        // LBTerm: only the dirty lines in the viewport and the cursor's are copied, the cursor's
        // once, under one lock; upstream copied the whole viewport to keep the dirty lines and
        // fetched the cursor's line again after them (lbterm_push.rs)
        let bonus_lines = crate::lbterm_push::lines_to_send(
            pane,
            viewport_range,
            &mut all_dirty_lines,
            cursor_position.y,
        );
        crate::lbterm_pushstats::mark(crate::lbterm_pushstats::Step::Copy);
        crate::lbterm_pushstats::carried(bonus_lines.len(), || {
            all_dirty_lines
                .iter()
                .map(|rows| (rows.end - rows.start) as u64)
                .sum()
        });

        self.cursor_position = cursor_position;
        self.title = title.clone();
        self.working_dir = working_dir.clone();
        self.dimensions = dims;
        self.mouse_grabbed = mouse_grabbed;
        self.alt_screen_active = alt_screen_active;

        let bonus_lines = bonus_lines.into();
        let changes = GetPaneRenderChangesResponse {
            pane_id: pane.pane_id(),
            mouse_grabbed,
            alt_screen_active,
            dirty_lines: all_dirty_lines.iter().cloned().collect(),
            dimensions: dims,
            cursor_position,
            title,
            bonus_lines,
            working_dir: working_dir.map(Into::into),
            input_serial: force_with_input_serial,
            seqno: self.seqno,
        };
        crate::lbterm_pushstats::mark(crate::lbterm_pushstats::Step::Cells);
        Some(changes)
    }

    /// LBTerm: what a push owed for a pane out of its client's view sends ahead of its alerts,
    /// its lines not worked out (lbterm_flow::Owed; lbterm/MUX.md, "Tabs out of view: output held
    /// at the server"). When `held`: the word that its lines are held as of the last push
    /// (PaneHeld at that push's seqno), if the pane's output since changed what a push carries
    /// (`read`, the push's read from that seqno without a line copied: a row, the cursor, the
    /// size or a flag; its title and folder go as the names push below). Output that changed
    /// none of that (ConPTY's title sent again as each program ends, a DECSET 2026 batch) holds
    /// nothing: the client's copy is current as of the read's seqno, which the next push starts
    /// from, and the hold ends with no word (PITFALLS 197). So does a hold whose output came in
    /// the last push, its notice after it (PITFALLS 189). When `names`: its title and folder
    /// (`read_names`), if either changed, in a push of them alone, with no lines and no rows named
    /// and the cursor, size, flags and seqno as the last push had them, which the client takes as
    /// a change of its title or folder alone (PITFALLS 163); but a pane whose size isn't the last
    /// push's (a resize answered and its asked push still to come) has a real push instead, which
    /// ends its hold, since the names push would give the client its old size back. The size
    /// alone ([`lbterm_same_size`]): the rest of the dimensions moves with every line a held pane
    /// scrolls, and compared whole they made every title of a pane printing out of view a real
    /// push, its hold ended and begun again a title (PITFALLS 242). Whether a real push is owed
    /// now ([`OutOfView::Push`])
    fn lbterm_out_of_view(
        &mut self,
        pane_id: PaneId,
        held: bool,
        names: bool,
        read: impl FnOnce(SequenceNo) -> mux::lbterm_push_read::PushRead,
        read_names: impl FnOnce() -> mux::lbterm_push_read::Names,
    ) -> OutOfView {
        let mut pdus = vec![];
        if held && self.lbterm_held.is_none() {
            let read = read(self.seqno);
            let shows_more = read.mouse_grabbed != self.mouse_grabbed
                || read.alt_screen_active != self.alt_screen_active
                || read.dimensions != self.dimensions
                || read.cursor_position != self.cursor_position
                || !read.changed.is_empty();
            if read.seqno > self.seqno && shows_more {
                self.lbterm_held = Some(self.seqno);
                pdus.push(Pdu::PaneHeld(PaneHeld {
                    pane_id,
                    seqno: self.seqno,
                }));
            } else {
                // Nothing a push compares has moved but the title or folder: the rows named
                // since are none, so the client's copy is the pane's as of this read
                self.seqno = self.seqno.max(read.seqno);
                if let Some(pending) = &self.lbterm_pending {
                    pending.lock().unwrap().current(pane_id);
                }
            }
        }
        if names {
            let mux::lbterm_push_read::Names {
                title,
                working_dir,
                dimensions,
            } = read_names();
            if !lbterm_same_size(&dimensions, &self.dimensions) {
                // The real push carries what the word would have said: none is sent, and none
                // is owed an answer
                self.lbterm_held = None;
                return OutOfView::Push;
            }
            if title != self.title || working_dir != self.working_dir {
                self.title = title.clone();
                self.working_dir = working_dir.clone();
                pdus.push(Pdu::GetPaneRenderChangesResponse(
                    GetPaneRenderChangesResponse {
                        pane_id,
                        mouse_grabbed: self.mouse_grabbed,
                        alt_screen_active: self.alt_screen_active,
                        dirty_lines: vec![],
                        dimensions: self.dimensions,
                        cursor_position: self.cursor_position,
                        title,
                        bonus_lines: Vec::new().into(),
                        working_dir: working_dir.map(Into::into),
                        input_serial: None,
                        seqno: self.seqno,
                    },
                ));
            }
        }
        OutOfView::Send(pdus)
    }
}

/// LBTerm: whether `a` and `b` are the same size: the columns and rows, the pixels they take and
/// the dpi, what a resize changes. Not where the screen's top is or how much scrollback there is
/// (`physical_top`, `scrollback_top`, `scrollback_rows`), which move with every line a pane
/// scrolls, nor `reverse_video`, a flag a names push carries as last pushed like the others
fn lbterm_same_size(a: &RenderableDimensions, b: &RenderableDimensions) -> bool {
    a.cols == b.cols
        && a.viewport_rows == b.viewport_rows
        && a.pixel_width == b.pixel_width
        && a.pixel_height == b.pixel_height
        && a.dpi == b.dpi
}

/// LBTerm: what a push owed for a pane out of its client's view sends (`lbterm_out_of_view`)
#[derive(Debug)]
enum OutOfView {
    /// These, ahead of its alerts, its lines still held
    Send(Vec<Pdu>),
    /// A push of its changes worked out now, as for a pane in view
    Push,
}

/// LBTerm: `answer`, run on the main thread once `pane_id` is current (Lachlan's 6b of
/// 2026-11-08; lbterm/MUX.md, "What reads a held pane's lines"). In a window's own listener
/// (gui-sock) the panes are mux client panes, and one its server holds has its push asked and
/// applied first, awaited in a task of its own (the wait isn't Send, so it can't be awaited in
/// the request's `spawn_into_main_thread` task), up to config's `HELD_READ_WAIT`, the GUI thread
/// running its other work meanwhile; any other pane, and every pane in a mux server, is its own:
/// a lookup, and `answer` at once
fn lbterm_when_current(pane_id: PaneId, answer: impl FnOnce() + 'static) {
    match Mux::get().get_pane(pane_id).and_then(|pane| pane.lbterm_current()) {
        None => answer(),
        Some(wait) => promise::spawn::spawn(async move {
            wait.await;
            answer();
        })
        .detach(),
    }
}

fn maybe_push_pane_changes(
    pane: &Arc<dyn Pane>,
    sender: PduSender,
    per_pane: Arc<Mutex<PerPane>>,
) -> anyhow::Result<()> {
    let mut per_pane = per_pane.lock().unwrap();
    push_pane_changes(pane, &mut |pdu| sender.send(pdu), &mut *per_pane, true)
}

/// LBTerm: maybe_push_pane_changes' body, sending to `send`: a PduSender's channel, or the
/// connection's outbox for the pushes it owes (lbterm_flow.rs). `changes`: the pane's changes
/// are worked out and sent ahead of its alerts; without, only the alerts and the first push's
/// state go, for an owe of alerts that carry what they say (lbterm_flow::Owed)
fn push_pane_changes(
    pane: &Arc<dyn Pane>,
    send: &mut dyn FnMut(DecodedPdu) -> anyhow::Result<()>,
    per_pane: &mut PerPane,
    changes: bool,
) -> anyhow::Result<()> {
    if changes {
        if let Some(resp) = per_pane.compute_changes(pane, None) {
            send(DecodedPdu {
                pdu: Pdu::GetPaneRenderChangesResponse(resp),
                serial: 0,
            })?;
        }
    }

    let config = config::configuration();
    if per_pane.config_generation != config.generation() {
        per_pane.config_generation = config.generation();
        // If the config changed, it may have changed colors
        // in the palette that we need to push down, so we
        // synthesize a palette change notification to let
        // the client know
        per_pane.notifications.push(Alert::PaletteChanged);
        per_pane.sent_initial_palette = true;
    }

    if !per_pane.sent_initial_palette {
        per_pane.notifications.push(Alert::PaletteChanged);
        per_pane.sent_initial_palette = true;
    }

    if !per_pane.sent_initial_state {
        // LBTerm: a client hears only of changes, so one that attaches to a
        // running pane (a GUI started again while its panes live on here)
        // would have none of its user vars (Claude Code's claude_status and
        // claude_session), progress or OSC 7501 records until each next
        // changed. Send what the pane has, ahead of anything queued.
        per_pane.sent_initial_state = true;
        let mut initial: Vec<Alert> = pane
            .copy_user_vars()
            .into_iter()
            .map(|(name, value)| Alert::SetUserVar { name, value })
            .collect();
        let progress = pane.get_progress();
        if progress != Progress::None {
            initial.push(Alert::Progress(progress));
        }
        // Only records there are: every new pane has none, and an empty SetProgramStatus
        // for a pane the GUI doesn't know yet made it resync
        if !pane.get_program_status().is_empty() {
            initial.push(Alert::ProgramStatus);
        }
        per_pane.notifications.splice(0..0, initial);
    }

    let mut sent_program_status = false;
    // LBTerm: taken out whole, as drain(..) left it, so a palette sent can be noted as it goes
    let notifications = std::mem::take(&mut per_pane.notifications);
    for alert in notifications {
        match alert {
            Alert::ProgramStatus => {
                // LBTerm: the alert only says the records changed, and a
                // client can't read them from here, so send the records.
                // Once per push: they're all of them each time.
                if !sent_program_status {
                    sent_program_status = true;
                    send(DecodedPdu {
                        pdu: Pdu::SetProgramStatus(SetProgramStatus {
                            pane_id: pane.pane_id(),
                            records: pane.get_program_status(),
                        }),
                        serial: 0,
                    })?;
                }
            }
            Alert::PaletteChanged => {
                // LBTerm: only a palette this client hasn't got. A config save had every pane's
                // sent twice: once as the answer to the GUI's own SetPalette, and again as the
                // server read the same file itself (its generation moved above), each making
                // the GUI fetch the pane's lines again and throw away its shapes
                // (lbterm/MUX.md, "What a config save costs")
                let palette = pane.palette();
                if per_pane.sent_palette.as_ref() == Some(&palette) {
                    continue;
                }
                per_pane.sent_palette = Some(palette.clone());
                send(DecodedPdu {
                    pdu: Pdu::SetPalette(SetPalette {
                        pane_id: pane.pane_id(),
                        palette,
                    }),
                    serial: 0,
                })?;
            }
            alert => {
                send(DecodedPdu {
                    pdu: Pdu::NotifyAlert(NotifyAlert {
                        pane_id: pane.pane_id(),
                        alert,
                    }),
                    serial: 0,
                })?;
            }
        }
    }
    Ok(())
}

pub struct SessionHandler {
    to_write_tx: PduSender,
    per_pane: HashMap<TabId, Arc<Mutex<PerPane>>>,
    client_id: Option<Arc<ClientId>>,
    proxy_client_id: Option<ClientId>,
    /// LBTerm: the pane this connection last gave input to, pushed ahead of the others it's owed
    /// (lbterm_flow.rs), so a key's echo never waits behind other panes' output
    lbterm_input_pane: Option<PaneId>,
    /// LBTerm: this connection, to the resize book, which keeps from it the TabResized its own
    /// Resizes alone make (lbterm_resize.rs, is_own)
    lbterm_conn: crate::lbterm_resize::Conn,
    /// LBTerm: what this connection's pushes cost, logged by a test window's server
    /// (lbterm_pushstats.rs)
    lbterm_stats: crate::lbterm_pushstats::PushStats,
    /// LBTerm: what this connection owes its client (lbterm_flow.rs), which its mux subscriber
    /// and loop share (dispatch.rs), and the panes the client says it shows go in
    /// (SetShownPanes)
    lbterm_pending: Arc<Mutex<crate::lbterm_flow::Pending>>,
}

impl Drop for SessionHandler {
    fn drop(&mut self) {
        if let Some(client_id) = self.client_id.take() {
            let mux = Mux::get();
            mux.unregister_client(&client_id);
        }
    }
}

impl SessionHandler {
    pub fn new(to_write_tx: PduSender) -> Self {
        Self {
            to_write_tx,
            per_pane: HashMap::new(),
            client_id: None,
            proxy_client_id: None,
            lbterm_input_pane: None,
            lbterm_conn: crate::lbterm_resize::new_conn(),
            lbterm_stats: crate::lbterm_pushstats::PushStats::new(),
            lbterm_pending: Arc::default(),
        }
    }

    /// LBTerm: what this connection owes its client, for its mux subscriber and loop
    /// (dispatch.rs, lbterm_flow.rs)
    pub fn lbterm_pending(&self) -> Arc<Mutex<crate::lbterm_flow::Pending>> {
        Arc::clone(&self.lbterm_pending)
    }

    /// LBTerm: this connection, as the resize book knows it (lbterm_resize.rs)
    pub fn lbterm_conn(&self) -> crate::lbterm_resize::Conn {
        self.lbterm_conn
    }

    /// LBTerm: when the cli types into `pane_id` (`cli send-text`, from the Conductor, Relay
    /// or the Claude project), say so on the pane with the `lbterm_typed` user var, the time in
    /// milliseconds: the tab rail lights the tab's pad. A GUI's own typing doesn't.
    fn lbterm_mark_typed(&self, pane_id: PaneId) {
        if !mux::lbterm_typed::by_a_program(self.client_id.as_deref()) {
            return;
        }
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        Mux::get().notify(MuxNotification::Alert {
            pane_id,
            alert: Alert::SetUserVar {
                name: mux::lbterm_typed::USER_VAR.to_string(),
                value: at.to_string(),
            },
        });
    }

    /// LBTerm: whether this connection gets `pane_id`'s changes pushed as they come: a window's
    /// does, and a program's only for a pane it asked about (mux::lbterm_typed::gets_changes)
    pub(crate) fn gets_changes(&self, pane_id: PaneId) -> bool {
        mux::lbterm_typed::gets_changes(self.client_id.as_deref(), self.per_pane.contains_key(&pane_id))
    }

    /// LBTerm: what input to `pane_id` from this connection pushes the pane's changes to: a
    /// window's own, and a program's only for a pane it asked about (gets_changes). A `cli
    /// send-text` (the Conductor, Relay) was sent the whole pane, every line of its viewport,
    /// its palette and all its user vars and records, before its answer, and threw it away; and
    /// its connection got the pane's every push after that
    fn lbterm_per_pane_for_input(&mut self, pane_id: PaneId) -> Option<Arc<Mutex<PerPane>>> {
        if self.gets_changes(pane_id) {
            Some(self.per_pane(pane_id))
        } else {
            None
        }
    }

    /// LBTerm: input to `pane_id` from a connection that gets its changes makes it the pane
    /// pushed first (lbterm_input_pane)
    fn lbterm_note_input(&mut self, pane_id: PaneId, per_pane: &Option<Arc<Mutex<PerPane>>>) {
        if per_pane.is_some() {
            self.lbterm_input(pane_id);
        }
    }

    /// LBTerm: the client gave `pane_id` input: it's the pane pushed first from now on, and until
    /// a frame of the client's has read a push of it made since, its round pushes wait for no
    /// frame's word, so the echo goes as it comes (lbterm_flow::Pending::input; lbterm/MUX.md,
    /// "A shown pane's pace")
    pub(crate) fn lbterm_input(&mut self, pane_id: PaneId) {
        self.lbterm_input_pane = Some(pane_id);
        self.lbterm_pending
            .lock()
            .unwrap()
            .input(pane_id, Instant::now());
    }

    /// LBTerm: the client says which push of each pane named its frames have read (codec's
    /// PanesDrawn): each pane's next round push can go, if it waited for that word
    /// (lbterm_flow::Pending::drawn), and the push log counts the ones that waited
    pub(crate) fn lbterm_drawn(&mut self, panes: &[PaneDrawn]) {
        let now = Instant::now();
        let mut pending = self.lbterm_pending.lock().unwrap();
        for pane in panes {
            if let Some(waited) = pending.drawn(pane.pane_id, pane.seqno, now) {
                self.lbterm_stats.waited(pane.pane_id, waited);
            }
        }
    }

    pub(crate) fn per_pane(&mut self, pane_id: PaneId) -> Arc<Mutex<PerPane>> {
        let pending = &self.lbterm_pending;
        Arc::clone(self.per_pane.entry(pane_id).or_insert_with(|| {
            Arc::new(Mutex::new(PerPane {
                lbterm_pending: Some(Arc::clone(pending)),
                ..PerPane::default()
            }))
        }))
    }

    /// LBTerm: the pane this connection last gave input to (lbterm_flow::Pending::take_next)
    pub(crate) fn lbterm_input_pane(&self) -> Option<PaneId> {
        self.lbterm_input_pane
    }

    /// LBTerm: the push `pane_id` is owed, with the alerts it raised meanwhile, worked out now and
    /// encoded into the connection's outbox; nothing for a pane this connection doesn't get the
    /// changes of, or one that has gone (its PaneRemoved says so). Upstream's
    /// schedule_pane_push spawned a task for each notification that did this and queued what it
    /// found (lbterm_flow.rs)
    pub(crate) fn lbterm_push(
        &mut self,
        pane_id: PaneId,
        owed: crate::lbterm_flow::Owed,
        outbox: &mut crate::lbterm_flow::Outbox,
    ) -> anyhow::Result<()> {
        if !self.gets_changes(pane_id) {
            return Ok(());
        }
        let pane = match Mux::get().get_pane(pane_id) {
            Some(pane) => pane,
            None => return Ok(()),
        };
        self.lbterm_stats.begin();
        let pushed = self.lbterm_push_pane(&pane, owed, outbox);
        self.lbterm_stats.end(pane_id);
        pushed
    }

    /// LBTerm: lbterm_push's body, for `pane`. A pane whose lines this connection holds sends no
    /// changes but the word of its hold and its title and folder alone, ahead of its alerts
    /// (lbterm_flow.rs; lbterm/MUX.md, "Tabs out of view: output held at the server")
    fn lbterm_push_pane(
        &mut self,
        pane: &Arc<dyn Pane>,
        owed: crate::lbterm_flow::Owed,
        outbox: &mut crate::lbterm_flow::Outbox,
    ) -> anyhow::Result<()> {
        let pane_id = pane.pane_id();
        let per_pane = self.per_pane(pane_id);
        let mut per_pane = per_pane.lock().unwrap();
        for alert in owed.alerts {
            crate::lbterm_flow::note_alert(&mut per_pane.notifications, alert);
        }
        // A pane this connection has had no changes of yet gets them whatever it's owed for
        let mut changes = owed.changes || per_pane.seqno == 0;
        // The seqno of the push of the pane's changes sent, if one is (What could go 12)
        let mut pushed: Option<SequenceNo> = None;
        let mut send = |pdu: DecodedPdu| {
            if let Pdu::GetPaneRenderChangesResponse(push) = &pdu.pdu {
                pushed = Some(push.seqno);
            }
            let before = outbox.len();
            let added = outbox.add(&pdu);
            crate::lbterm_pushstats::encoded(
                outbox.len().saturating_sub(before),
                matches!(pdu.pdu, Pdu::GetPaneRenderChangesResponse(_)),
            );
            added
        };
        if !changes {
            match per_pane.lbterm_out_of_view(
                pane_id,
                owed.held,
                owed.names,
                |since| pane.lbterm_read_for_push(since),
                || pane.lbterm_read_names(),
            ) {
                OutOfView::Send(pdus) => {
                    crate::lbterm_pushstats::alerts_alone();
                    for pdu in pdus {
                        match &pdu {
                            Pdu::PaneHeld(_) => crate::lbterm_pushstats::held(),
                            _ => crate::lbterm_pushstats::names(),
                        }
                        send(DecodedPdu { pdu, serial: 0 })?;
                    }
                }
                OutOfView::Push => changes = true,
            }
        }
        push_pane_changes(pane, &mut send, &mut *per_pane, changes)?;
        // A push of a shown pane's changes, on a connection whose client says what its frames
        // read, leaves the pane waiting for the word before its next round push (lbterm/MUX.md,
        // "A shown pane's pace"); a names push, sent without its changes, carries none
        if let (true, Some(seqno)) = (changes, pushed) {
            self.lbterm_pending
                .lock()
                .unwrap()
                .pushed_changes(pane_id, seqno, Instant::now());
        }
        Ok(())
    }

    /// LBTerm: the connection ended a round of pushes and waits for `wait`; a test window's
    /// server logs what its pushes cost once a second (lbterm_pushstats.rs)
    pub(crate) fn lbterm_round(&mut self, wait: crate::lbterm_flow::Wait) {
        if let Some(line) = self.lbterm_stats.round(wait) {
            log::info!(target: "lbterm::push", "{}", line);
        }
    }

    pub fn process_one(&mut self, decoded: DecodedPdu) {
        let start = Instant::now();
        let sender = self.to_write_tx.clone();
        let serial = decoded.serial;

        if let Some(client_id) = &self.client_id {
            if decoded.pdu.is_user_input() {
                Mux::get().client_had_input(client_id);
            }
        }

        let send_response = move |result: anyhow::Result<Pdu>| {
            let pdu = match result {
                Ok(pdu) => pdu,
                Err(err) => Pdu::ErrorResponse(ErrorResponse {
                    reason: format!("Error: {err:#}"),
                }),
            };
            log::trace!("{} processing time {:?}", serial, start.elapsed());
            sender.send(DecodedPdu { pdu, serial }).ok();
        };

        fn catch<F, SND>(f: F, send_response: SND)
        where
            F: FnOnce() -> anyhow::Result<Pdu>,
            SND: Fn(anyhow::Result<Pdu>),
        {
            send_response(f());
        }

        match decoded.pdu {
            Pdu::Ping(Ping {}) => send_response(Ok(Pdu::Pong(Pong {}))),
            Pdu::SetWindowWorkspace(SetWindowWorkspace {
                window_id,
                workspace,
            }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let mut window = mux
                                .get_window_mut(window_id)
                                .ok_or_else(|| anyhow!("window {} is invalid", window_id))?;
                            window.set_workspace(&workspace);
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }
            Pdu::SetClientId(SetClientId {
                mut client_id,
                is_proxy,
            }) => {
                if is_proxy {
                    if self.proxy_client_id.is_none() {
                        // Copy proxy identity, but don't assign it to the mux;
                        // we'll use it to annotate the actual clients own
                        // identity when they send it
                        self.proxy_client_id.replace(client_id);
                    }
                } else {
                    // If this session is a proxy, override the incoming id with
                    // the proxy information so that it is clear what is going
                    // on from the `wezterm cli list-clients` information
                    if let Some(proxy_id) = &self.proxy_client_id {
                        client_id.ssh_auth_sock = proxy_id.ssh_auth_sock.clone();
                        // Note that this `via proxy pid` string is coupled
                        // with the logic in mux/src/ssh_agent
                        client_id.hostname =
                            format!("{} (via proxy pid {})", client_id.hostname, proxy_id.pid);
                    }

                    let client_id = Arc::new(client_id);
                    self.client_id.replace(client_id.clone());
                    spawn_into_main_thread(async move {
                        let mux = Mux::get();
                        mux.register_client(client_id);
                    })
                    .detach();
                }
                send_response(Ok(Pdu::UnitResponse(UnitResponse {})))
            }
            // LBTerm: the panes the client's windows show, in place of the set it said before,
            // noted in what the connection owes (lbterm_flow::Pending::set_shown), which owes the
            // push of each pane held there that it shows, ahead of the rest. Here, on the
            // connection's own task, rather than a task of its own: it reads nothing of the mux,
            // and the round that follows this PDU pushes them (lbterm/MUX.md, "Tabs out of view:
            // output held at the server")
            Pdu::SetShownPanes(SetShownPanes { panes }) => {
                self.lbterm_pending.lock().unwrap().set_shown(&panes);
                send_response(Ok(Pdu::UnitResponse(UnitResponse {})))
            }
            // LBTerm: which push of each shown pane the window's frames have read, a notice the
            // window waits on no answer for and sends at serial 0, so none goes back: one would
            // reach the window as a push it can't hand on and end the connection (PITFALLS 227).
            // Noted on the connection's own task, as SetShownPanes is: the round that follows
            // this PDU pushes each pane that waited for the word (lbterm/MUX.md, "A shown pane's
            // pace")
            Pdu::PanesDrawn(PanesDrawn { panes }) => self.lbterm_drawn(&panes),
            // LBTerm: which tabs are parked (lbterm/MUX.md, "Parked tabs"). Noted first, on the
            // connection's own task, that the client knows TabParked, so a park from now on is
            // pushed to it; the list read on the main thread, where the tabs are parked. A park
            // between the two reaches it twice, the same either way
            Pdu::GetParkedTabs(GetParkedTabs {}) => {
                self.lbterm_pending.lock().unwrap().asked_parked();
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let tabs = mux::lbterm_parked::parked_tabs(&mux);
                            Ok(Pdu::ParkedTabs(ParkedTabs { tabs }))
                        },
                        send_response,
                    )
                })
                .detach();
            }
            // LBTerm: a window parks or unparks a tab: set here, where the tab is, and every
            // window that asked is told, the one that parked it among them (dispatch.rs)
            Pdu::TabParked(TabParked { tab_id, parked }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let tab = mux
                                .get_tab(tab_id)
                                .ok_or_else(|| anyhow!("no such tab {tab_id}"))?;
                            tab.lbterm_set_parked(parked);
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }
            Pdu::SetFocusedPane(SetFocusedPane { pane_id }) => {
                let client_id = self.client_id.clone();
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let _identity = mux.with_identity(client_id);

                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow::anyhow!("pane {pane_id} not found"))?;

                            let (_domain_id, window_id, tab_id) = mux
                                .resolve_pane_id(pane_id)
                                .ok_or_else(|| anyhow::anyhow!("pane {pane_id} not found"))?;
                            {
                                let mut window =
                                    mux.get_window_mut(window_id).ok_or_else(|| {
                                        anyhow::anyhow!("window {window_id} not found")
                                    })?;
                                let tab_idx =
                                    window.get_tab_idx_for_id(tab_id).ok_or_else(|| {
                                        anyhow::anyhow!(
                                            "tab {tab_id} isn't really in window {window_id}!?"
                                        )
                                    })?;
                                window.remember_and_set_active_tab_idx(tab_idx);
                            }
                            let tab = mux
                                .get_tab(tab_id)
                                .ok_or_else(|| anyhow::anyhow!("tab {tab_id} not found"))?;
                            tab.set_active_pane(&pane);

                            mux.record_focus_for_current_identity(pane_id);
                            mux.notify(mux::MuxNotification::PaneFocused(pane_id));

                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }
            Pdu::GetClientList(GetClientList) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let clients = mux.iter_clients();
                            Ok(Pdu::GetClientListResponse(GetClientListResponse {
                                clients,
                            }))
                        },
                        send_response,
                    )
                })
                .detach();
            }
            Pdu::ListPanes(ListPanes {}) => {
                // LBTerm: run once no pane is being resized, so no pane is listed at a size its
                // resize thread hasn't reached (lbterm_resize.rs, Resizes::after_all)
                let task = move || spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let mut tabs = vec![];
                            let mut tab_titles = vec![];
                            let mut window_titles = HashMap::new();
                            for window_id in mux.iter_windows().into_iter() {
                                let window = mux.get_window(window_id).unwrap();
                                // LBTerm: a title its server gave a GUI's window and not yet set
                                // on it is read through (mux's lbterm_window_titles)
                                window_titles
                                    .insert(window_id, mux::lbterm_window_titles::title(&window));
                                for tab in window.iter_tabs() {
                                    tabs.push(tab.codec_pane_tree());
                                    tab_titles.push(tab.get_title());
                                }
                            }
                            log::trace!("ListPanes {tabs:#?} {tab_titles:?}");
                            Ok(Pdu::ListPanesResponse(ListPanesResponse {
                                tabs,
                                tab_titles,
                                window_titles,
                            }))
                        },
                        send_response,
                    )
                })
                .detach();
                crate::lbterm_resize::resizes().after_all(Box::new(task));
            }

            Pdu::RenameWorkspace(RenameWorkspace {
                old_workspace,
                new_workspace,
            }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            mux.rename_workspace(&old_workspace, &new_workspace);
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    );
                })
                .detach();
            }

            Pdu::WriteToPane(WriteToPane { pane_id, data }) => {
                self.lbterm_mark_typed(pane_id);
                let sender = self.to_write_tx.clone();
                let per_pane = self.lbterm_per_pane_for_input(pane_id);
                self.lbterm_note_input(pane_id, &per_pane);
                // LBTerm: run after the pane's resizes under way, as upstream's main thread took it
                // behind them (lbterm_resize.rs, Resizes::after)
                let task = move || spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            pane.writer().write_all(&data)?;
                            if let Some(per_pane) = per_pane {
                                maybe_push_pane_changes(&pane, sender, per_pane)?;
                            }
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    );
                })
                .detach();
                crate::lbterm_resize::resizes().after(pane_id, Box::new(task));
            }
            Pdu::EraseScrollbackRequest(EraseScrollbackRequest {
                pane_id,
                erase_mode,
            }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            pane.erase_scrollback(erase_mode);
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    );
                })
                .detach();
            }
            Pdu::KillPane(KillPane { pane_id }) => {
                let sender = self.to_write_tx.clone();
                let per_pane = self.lbterm_per_pane_for_input(pane_id);
                // LBTerm: run once no pane is being resized: the tab left resizes its other panes
                // on this thread (lbterm_resize.rs, Resizes::after_all)
                let task = move || spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            pane.kill();
                            mux.remove_pane(pane_id);
                            if let Some(per_pane) = per_pane {
                                maybe_push_pane_changes(&pane, sender, per_pane)?;
                            }
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    );
                })
                .detach();
                crate::lbterm_resize::resizes().after_all(Box::new(task));
            }
            Pdu::SendPaste(SendPaste { pane_id, data }) => {
                self.lbterm_mark_typed(pane_id);
                let sender = self.to_write_tx.clone();
                let per_pane = self.lbterm_per_pane_for_input(pane_id);
                self.lbterm_note_input(pane_id, &per_pane);
                // LBTerm: run after the pane's resizes under way, as upstream's main thread took it
                // behind them (lbterm_resize.rs, Resizes::after)
                let task = move || spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            pane.send_paste(&data)?;
                            if let Some(per_pane) = per_pane {
                                maybe_push_pane_changes(&pane, sender, per_pane)?;
                            }
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
                crate::lbterm_resize::resizes().after(pane_id, Box::new(task));
            }

            Pdu::SearchScrollbackRequest(SearchScrollbackRequest {
                pane_id,
                pattern,
                range,
                limit,
            }) => {
                use mux::pane::Pattern;

                async fn do_search(
                    pane_id: TabId,
                    pattern: Pattern,
                    range: std::ops::Range<StableRowIndex>,
                    limit: Option<u32>,
                ) -> anyhow::Result<Pdu> {
                    let mux = Mux::get();
                    let pane = mux
                        .get_pane(pane_id)
                        .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;

                    pane.search(pattern, range, limit).await.map(|results| {
                        Pdu::SearchScrollbackResponse(SearchScrollbackResponse { results })
                    })
                }

                spawn_into_main_thread(async move {
                    promise::spawn::spawn(async move {
                        let result = do_search(pane_id, pattern, range, limit).await;
                        send_response(result);
                    })
                    .detach();
                })
                .detach();
            }

            Pdu::SetPaneZoomed(SetPaneZoomed {
                containing_tab_id,
                pane_id,
                zoomed,
            }) => {
                // LBTerm: run once no pane is being resized: a zoom resizes the pane on this
                // thread (lbterm_resize.rs, Resizes::after_all)
                let task = move || spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            let tab = mux
                                .get_tab(containing_tab_id)
                                .ok_or_else(|| anyhow!("no such tab {}", containing_tab_id))?;
                            match tab.get_zoomed_pane() {
                                Some(p) => {
                                    let is_zoomed = p.pane_id() == pane_id;
                                    if is_zoomed != zoomed {
                                        tab.set_zoomed(false);
                                        if zoomed {
                                            tab.set_active_pane(&pane);
                                            tab.set_zoomed(zoomed);
                                        }
                                    }
                                }
                                None => {
                                    if zoomed {
                                        tab.set_active_pane(&pane);
                                        tab.set_zoomed(zoomed);
                                    }
                                }
                            }
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
                crate::lbterm_resize::resizes().after_all(Box::new(task));
            }

            Pdu::GetPaneDirection(GetPaneDirection { pane_id, direction }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let (_domain_id, _window_id, tab_id) = mux
                                .resolve_pane_id(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            let tab = mux
                                .get_tab(tab_id)
                                .ok_or_else(|| anyhow!("no such tab {}", tab_id))?;
                            let panes = tab.iter_panes_ignoring_zoom();
                            let pane_id = tab
                                .get_pane_direction(direction, true)
                                .map(|pane_index| panes[pane_index].pane.pane_id());

                            Ok(Pdu::GetPaneDirectionResponse(GetPaneDirectionResponse {
                                pane_id,
                            }))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::ActivatePaneDirection(ActivatePaneDirection { pane_id, direction }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let (_domain_id, _window_id, tab_id) = mux
                                .resolve_pane_id(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            let tab = mux
                                .get_tab(tab_id)
                                .ok_or_else(|| anyhow!("no such tab {}", tab_id))?;
                            tab.activate_pane_direction(direction);
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::Resize(Resize {
                containing_tab_id,
                pane_id,
                size,
            }) => {
                // LBTerm: the pane resized on a thread of its own, to the newest size asked for
                // it, this answered once it has that size (lbterm_resize.rs)
                crate::lbterm_resize::resizes().ask(
                    pane_id,
                    containing_tab_id,
                    size,
                    self.lbterm_conn,
                    Box::new(send_response),
                );
            }

            Pdu::SendKeyDown(SendKeyDown {
                pane_id,
                event,
                input_serial,
            }) => {
                let sender = self.to_write_tx.clone();
                let per_pane = self.per_pane(pane_id);
                self.lbterm_input(pane_id);
                // LBTerm: run after the pane's resizes under way, as upstream's main thread took it
                // behind them (lbterm_resize.rs, Resizes::after)
                let task = move || spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            pane.key_down(event.key, event.modifiers)?;

                            // For a key press, we want to always send back the
                            // cursor position so that the predictive echo doesn't
                            // leave the cursor in the wrong place
                            let mut per_pane = per_pane.lock().unwrap();
                            if let Some(resp) = per_pane.compute_changes(&pane, Some(input_serial))
                            {
                                sender.send(DecodedPdu {
                                    pdu: Pdu::GetPaneRenderChangesResponse(resp),
                                    serial: 0,
                                })?;
                            }
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
                crate::lbterm_resize::resizes().after(pane_id, Box::new(task));
            }
            Pdu::SendMouseEvent(SendMouseEvent { pane_id, event }) => {
                // LBTerm: no push of the pane's changes after it, where upstream worked them out
                // for every event (maybe_push_pane_changes: the pane's getters, each under its
                // terminal's lock, its changed rows and its cwd), on this, the server's only
                // thread, while the pointer moved over a program with the mouse: about 0.1 ms a
                // move (perf/mux-client). A mouse event changes nothing a push carries: the
                // terminal only writes the program a report (term's mouse.rs), and what the
                // program draws for it comes back as its output, which the connection is owed a
                // push for (lbterm_flow.rs), this pane first, as input's
                let per_pane = self.lbterm_per_pane_for_input(pane_id);
                self.lbterm_note_input(pane_id, &per_pane);
                // LBTerm: run after the pane's resizes under way, as upstream's main thread took it
                // behind them (lbterm_resize.rs, Resizes::after)
                let task = move || spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            pane.mouse_event(event)?;
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
                crate::lbterm_resize::resizes().after(pane_id, Box::new(task));
            }

            Pdu::SpawnV2(spawn) => {
                let client_id = self.client_id.clone();
                spawn_into_main_thread(async move {
                    schedule_domain_spawn_v2(spawn, true, send_response, client_id);
                })
                .detach();
            }

            Pdu::SpawnNoActivate(SpawnNoActivate { spawn }) => {
                let client_id = self.client_id.clone();
                spawn_into_main_thread(async move {
                    schedule_domain_spawn_v2(spawn, false, send_response, client_id);
                })
                .detach();
            }

            Pdu::SplitPane(split) => {
                let client_id = self.client_id.clone();
                // LBTerm: run once no pane is being resized: a split resizes the pane it splits
                // on this thread (lbterm_resize.rs, Resizes::after_all)
                let task = move || spawn_into_main_thread(async move {
                    schedule_split_pane(split, send_response, client_id);
                })
                .detach();
                crate::lbterm_resize::resizes().after_all(Box::new(task));
            }

            Pdu::MovePaneToNewTab(request) => {
                let client_id = self.client_id.clone();
                // LBTerm: run once no pane is being resized: the tab left resizes its other panes
                // on this thread (lbterm_resize.rs, Resizes::after_all)
                let task = move || spawn_into_main_thread(async move {
                    schedule_move_pane(request, send_response, client_id);
                })
                .detach();
                crate::lbterm_resize::resizes().after_all(Box::new(task));
            }

            Pdu::GetPaneRenderableDimensions(GetPaneRenderableDimensions { pane_id }) => {
                // LBTerm: run after the pane's resizes under way, whose size it says
                // (lbterm_resize.rs, Resizes::after); in a window's own listener (`cli get-text`
                // through gui-sock), a pane its server holds brought current first
                let task = move || spawn_into_main_thread(async move {
                    lbterm_when_current(pane_id, move || catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            let cursor_position = pane.get_cursor_position();
                            let dimensions = pane.get_dimensions();
                            Ok(Pdu::GetPaneRenderableDimensionsResponse(
                                GetPaneRenderableDimensionsResponse {
                                    pane_id,
                                    cursor_position,
                                    dimensions,
                                },
                            ))
                        },
                        send_response,
                    ));
                })
                .detach();
                crate::lbterm_resize::resizes().after(pane_id, Box::new(task));
            }

            Pdu::GetPaneRenderChanges(GetPaneRenderChanges { pane_id, .. }) => {
                let sender = self.to_write_tx.clone();
                let per_pane = self.per_pane(pane_id);
                // LBTerm: run after the pane's resizes under way, as upstream's main thread took it
                // behind them (lbterm_resize.rs, Resizes::after)
                let task = move || spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let is_alive = match mux.get_pane(pane_id) {
                                Some(pane) => {
                                    maybe_push_pane_changes(&pane, sender, per_pane)?;
                                    true
                                }
                                None => false,
                            };
                            Ok(Pdu::LivenessResponse(LivenessResponse {
                                pane_id,
                                is_alive,
                            }))
                        },
                        send_response,
                    )
                })
                .detach();
                crate::lbterm_resize::resizes().after(pane_id, Box::new(task));
            }

            Pdu::GetLines(GetLines { pane_id, lines }) => {
                // LBTerm: run after the pane's resizes under way, as upstream's main thread took it
                // behind them (lbterm_resize.rs, Resizes::after); a pane a window's own listener
                // reads that its server holds brought current first, as for its size
                let task = move || spawn_into_main_thread(async move {
                    lbterm_when_current(pane_id, move || catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            let mut lines_and_indices = vec![];

                            for range in lines {
                                let (first_row, lines) = pane.get_lines(range);
                                for (idx, mut line) in lines.into_iter().enumerate() {
                                    let stable_row = first_row + idx as StableRowIndex;
                                    line.compress_for_scrollback();
                                    lines_and_indices.push((stable_row, line));
                                }
                            }
                            Ok(Pdu::GetLinesResponse(GetLinesResponse {
                                pane_id,
                                lines: lines_and_indices.into(),
                            }))
                        },
                        send_response,
                    ));
                })
                .detach();
                crate::lbterm_resize::resizes().after(pane_id, Box::new(task));
            }

            Pdu::GetImageCell(GetImageCell {
                pane_id,
                line_idx,
                cell_idx,
                data_hash,
            }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let mut data = None;

                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;

                            let (_, lines) = pane.get_lines(line_idx..line_idx + 1);
                            'found_data: for line in lines {
                                if let Some(cell) = line.get_cell(cell_idx) {
                                    if let Some(images) = cell.attrs().images() {
                                        for im in images {
                                            if im.image_data().hash() == data_hash {
                                                data.replace(im.image_data().clone());
                                                break 'found_data;
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(Pdu::GetImageCellResponse(GetImageCellResponse {
                                pane_id,
                                data,
                            }))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::GetCodecVersion(_) => {
                match std::env::current_exe().context("resolving current_exe") {
                    Err(err) => send_response(Err(err)),
                    Ok(executable_path) => {
                        send_response(Ok(Pdu::GetCodecVersionResponse(GetCodecVersionResponse {
                            codec_vers: CODEC_VERSION,
                            version_string: config::wezterm_version().to_owned(),
                            executable_path,
                            config_file_path: std::env::var_os("WEZTERM_CONFIG_FILE")
                                .map(Into::into),
                        })))
                    }
                }
            }

            Pdu::GetTlsCreds(_) => {
                catch(
                    move || {
                        let client_cert_pem = PKI.generate_client_cert()?;
                        let ca_cert_pem = PKI.ca_pem_string()?;
                        Ok(Pdu::GetTlsCredsResponse(GetTlsCredsResponse {
                            client_cert_pem,
                            ca_cert_pem,
                        }))
                    },
                    send_response,
                );
            }
            Pdu::WindowTitleChanged(WindowTitleChanged { window_id, title }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let mut window = mux
                                .get_window_mut(window_id)
                                .ok_or_else(|| anyhow!("no such window {window_id}"))?;

                            window.set_title(&title);

                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }
            Pdu::TabTitleChanged(TabTitleChanged { tab_id, title }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let tab = mux
                                .get_tab(tab_id)
                                .ok_or_else(|| anyhow!("no such tab {tab_id}"))?;

                            tab.set_title(&title);

                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }
            Pdu::SetPalette(SetPalette { pane_id, palette }) => {
                let sender = self.to_write_tx.clone();
                let per_pane = self.per_pane(pane_id);
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            // LBTerm: its clients hear of it only if it changed the pane's
                            // palette (maybe_push_pane_changes sends each a palette once too)
                            let before = pane.palette();

                            match pane.get_config() {
                                Some(config) => match config.downcast_ref::<TermConfig>() {
                                    Some(tc) => tc.set_client_palette(palette),
                                    None => {
                                        log::error!(
                                            "pane {pane_id} doesn't \
                                            have TermConfig as its config! \
                                            Ignoring client palette update"
                                        );
                                    }
                                },
                                None => {
                                    let config = TermConfig::new();
                                    config.set_client_palette(palette);
                                    pane.set_config(Arc::new(config));
                                }
                            }

                            if pane.palette() != before {
                                mux.notify(MuxNotification::Alert {
                                    pane_id,
                                    alert: Alert::PaletteChanged,
                                });
                            } else {
                                // LBTerm: and the client that sent it gets the pane pushed all
                                // the same. A new ClientPane sends its palette as it starts, and
                                // the push the notification brought was how a window attaching
                                // got a quiet background pane's user vars, progress and records
                                // (sent_initial_state) before the pane next printed: without it a
                                // GUI started again showed no tab's light or label
                                // (perf/mux.ps1)
                                maybe_push_pane_changes(&pane, sender, per_pane)?;
                            }

                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::AdjustPaneSize(AdjustPaneSize {
                pane_id,
                direction,
                amount,
            }) => {
                // LBTerm: run once no pane is being resized: a split's new size resizes its panes
                // on this thread (lbterm_resize.rs, Resizes::after_all)
                let task = move || spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let (_pane_domain_id, _window_id, tab_id) = mux
                                .resolve_pane_id(pane_id)
                                .ok_or_else(|| anyhow!("pane_id {} invalid", pane_id))?;

                            let tab = match mux.get_tab(tab_id) {
                                Some(tab) => tab,
                                None => {
                                    return Err(anyhow!(
                                        "Failed to retrieve tab with ID {}",
                                        tab_id
                                    ))
                                }
                            };

                            tab.adjust_pane_size(direction, amount);
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
                crate::lbterm_resize::resizes().after_all(Box::new(task));
            }

            // LBTerm: the rows holding a shell's prompt, for the scroll strip's marks: the rows
            // alone, never their lines (lbterm/SCROLL-STRIP.md, "A shell's marks")
            Pdu::GetPromptRows(GetPromptRows { pane_id, rows }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            let alt_screen = pane.is_alt_screen_active();
                            let (looked, rows) = mux::lbterm_prompts::prompt_rows(&*pane, rows);
                            Ok(Pdu::GetPromptRowsResponse(GetPromptRowsResponse {
                                pane_id,
                                looked,
                                rows,
                                alt_screen,
                            }))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::Invalid { .. } => send_response(Err(anyhow!("invalid PDU {:?}", decoded.pdu))),
            Pdu::Pong { .. }
            | Pdu::ListPanesResponse { .. }
            | Pdu::SetClipboard { .. }
            | Pdu::NotifyAlert { .. }
            | Pdu::SetProgramStatus { .. }
            | Pdu::SpawnResponse { .. }
            | Pdu::GetPaneRenderChangesResponse { .. }
            | Pdu::UnitResponse { .. }
            | Pdu::LivenessResponse { .. }
            | Pdu::GetPaneDirectionResponse { .. }
            | Pdu::SearchScrollbackResponse { .. }
            | Pdu::GetLinesResponse { .. }
            | Pdu::GetPromptRowsResponse { .. }
            | Pdu::ParkedTabs { .. }
            | Pdu::PaneHeld { .. }
            | Pdu::GetCodecVersionResponse { .. }
            | Pdu::WindowWorkspaceChanged { .. }
            | Pdu::GetTlsCredsResponse { .. }
            | Pdu::GetClientListResponse { .. }
            | Pdu::PaneRemoved { .. }
            | Pdu::PaneFocused { .. }
            | Pdu::TabResized { .. }
            | Pdu::GetImageCellResponse { .. }
            | Pdu::MovePaneToNewTabResponse { .. }
            | Pdu::TabAddedToWindow { .. }
            | Pdu::GetPaneRenderableDimensionsResponse { .. }
            | Pdu::ErrorResponse { .. } => {
                send_response(Err(anyhow!("expected a request, got {:?}", decoded.pdu)))
            }
        }
    }
}

// Dancing around a little bit here; we can't directly spawn_into_main_thread the domain_spawn
// function below because the compiler thinks that all of its locals then need to be Send.
// We need to shimmy through this helper to break that aspect of the compiler flow
// analysis and allow things to compile.
fn schedule_domain_spawn_v2<SND>(
    spawn: SpawnV2,
    activate: bool,
    send_response: SND,
    client_id: Option<Arc<ClientId>>,
) where
    SND: Fn(anyhow::Result<Pdu>) + 'static,
{
    promise::spawn::spawn(async move {
        send_response(domain_spawn_v2(spawn, activate, client_id).await)
    })
    .detach();
}

fn schedule_split_pane<SND>(split: SplitPane, send_response: SND, client_id: Option<Arc<ClientId>>)
where
    SND: Fn(anyhow::Result<Pdu>) + 'static,
{
    promise::spawn::spawn(async move { send_response(split_pane(split, client_id).await) })
        .detach();
}

async fn split_pane(split: SplitPane, client_id: Option<Arc<ClientId>>) -> anyhow::Result<Pdu> {
    let mux = Mux::get();
    let _identity = mux.with_identity(client_id);

    let (_pane_domain_id, window_id, tab_id) = mux
        .resolve_pane_id(split.pane_id)
        .ok_or_else(|| anyhow!("pane_id {} invalid", split.pane_id))?;

    let source = if let Some(move_pane_id) = split.move_pane_id {
        SplitSource::MovePane(move_pane_id)
    } else {
        SplitSource::Spawn {
            command: split.command,
            command_dir: split.command_dir,
        }
    };

    let (pane, size) = mux
        .split_pane(split.pane_id, split.split_request, source, split.domain)
        .await?;

    Ok::<Pdu, anyhow::Error>(Pdu::SpawnResponse(SpawnResponse {
        pane_id: pane.pane_id(),
        tab_id,
        window_id,
        size,
    }))
}

async fn domain_spawn_v2(
    spawn: SpawnV2,
    activate: bool,
    client_id: Option<Arc<ClientId>>,
) -> anyhow::Result<Pdu> {
    let mux = Mux::get();
    let _identity = mux.with_identity(client_id);

    let (tab, pane, window_id) = mux
        .spawn_tab_or_window_activating(
            spawn.window_id,
            spawn.domain,
            spawn.command,
            spawn.command_dir,
            spawn.size,
            None, // optional current pane_id
            spawn.workspace,
            None, // optional gui window position
            activate,
        )
        .await?;

    Ok::<Pdu, anyhow::Error>(Pdu::SpawnResponse(SpawnResponse {
        pane_id: pane.pane_id(),
        tab_id: tab.tab_id(),
        window_id,
        size: tab.get_size(),
    }))
}

fn schedule_move_pane<SND>(
    request: MovePaneToNewTab,
    send_response: SND,
    client_id: Option<Arc<ClientId>>,
) where
    SND: Fn(anyhow::Result<Pdu>) + 'static,
{
    promise::spawn::spawn(async move { send_response(move_pane(request, client_id).await) })
        .detach();
}

async fn move_pane(
    request: MovePaneToNewTab,
    client_id: Option<Arc<ClientId>>,
) -> anyhow::Result<Pdu> {
    let mux = Mux::get();
    let _identity = mux.with_identity(client_id);

    let (tab, window_id) = mux
        .move_pane_to_new_tab(
            request.pane_id,
            request.window_id,
            request.workspace_for_new_window,
        )
        .await?;

    Ok::<Pdu, anyhow::Error>(Pdu::MovePaneToNewTabResponse(MovePaneToNewTabResponse {
        tab_id: tab.tab_id(),
        window_id,
    }))
}

#[cfg(test)]
#[path = "lbterm_held_tests.rs"]
mod lbterm_held_tests;

#[cfg(test)]
mod lbterm_tests {
    use super::*;

    /// A handler whose answers land in a list
    fn handler() -> (SessionHandler, Arc<Mutex<Vec<DecodedPdu>>>) {
        let sent = Arc::new(Mutex::new(vec![]));
        let to = Arc::clone(&sent);
        let handler = SessionHandler::new(PduSender::new(move |pdu| {
            to.lock().unwrap().push(pdu);
            Ok(())
        }));
        (handler, sent)
    }

    /// SetShownPanes is answered at once with UnitResponse at its serial, so the window's
    /// request ends, and the set goes in the connection's Pending, which its loop shares; with
    /// no task and nothing read of the mux, so it holds with no mux at all
    #[test]
    fn the_panes_a_window_shows_are_noted_and_answered() {
        let (mut handler, sent) = handler();
        let pending = handler.lbterm_pending();
        assert!(pending.lock().unwrap().shows(5), "a window that hasn't said shows every pane");

        let panes = vec![
            ShownPane { pane_id: 5, drawn: None },
            ShownPane { pane_id: 6, drawn: None },
        ];
        handler.process_one(DecodedPdu {
            serial: 17,
            pdu: Pdu::SetShownPanes(SetShownPanes { panes }),
        });
        assert_eq!(
            sent.lock().unwrap().drain(..).collect::<Vec<_>>(),
            vec![DecodedPdu { serial: 17, pdu: Pdu::UnitResponse(UnitResponse {}) }]
        );
        let pending = pending.lock().unwrap();
        assert!(pending.says_shown());
        assert!(pending.shows(5) && pending.shows(6) && !pending.shows(7));
    }

    /// PanesDrawn is a notice: nothing goes back for it, at its serial (0) or any other, since a
    /// window's client thread would end the connection on an answer it holds no promise for
    /// (PITFALLS 227); the word goes in the connection's Pending, which paces the panes shown
    /// from then on, with no task and nothing read of the mux
    #[test]
    fn a_frames_word_is_noted_and_answered_with_nothing() {
        let (mut handler, sent) = handler();
        let pending = handler.lbterm_pending();
        handler.process_one(DecodedPdu {
            serial: 4,
            pdu: Pdu::SetShownPanes(SetShownPanes {
                panes: vec![ShownPane { pane_id: 5, drawn: None }],
            }),
        });
        sent.lock().unwrap().clear();

        let now = Instant::now();
        {
            let mut pending = pending.lock().unwrap();
            pending.output(5);
            pending.take_next(None);
            pending.pushed_changes(5, 30, now);
            pending.output(5);
            assert!(pending.take_next(None).is_some(), "not paced until the window says");
            pending.pushed_changes(5, 31, now);
        }
        handler.process_one(DecodedPdu {
            serial: 0,
            pdu: Pdu::PanesDrawn(PanesDrawn {
                panes: vec![PaneDrawn { pane_id: 5, seqno: 31 }, PaneDrawn { pane_id: 9, seqno: 2 }],
            }),
        });
        assert!(sent.lock().unwrap().is_empty(), "no answer");
        let mut pending = pending.lock().unwrap();
        pending.output(5);
        assert!(pending.take_next(None).is_some(), "nothing was waiting for the word");
        pending.pushed_changes(5, 32, now);
        pending.output(5);
        assert_eq!(pending.take_next(None), None, "paced from the word on");
    }

    /// PaneHeld goes from server to window only: one sent here is answered as any other
    /// PDU that isn't a request, and the connection goes on
    #[test]
    fn a_pane_held_sent_to_the_server_is_no_request() {
        let (mut handler, sent) = handler();
        handler.process_one(DecodedPdu {
            serial: 3,
            pdu: Pdu::PaneHeld(PaneHeld { pane_id: 1, seqno: 2 }),
        });
        let sent = sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].serial, 3);
        assert!(matches!(&sent[0].pdu, Pdu::ErrorResponse(e) if e.reason.contains("expected a request")));
    }
}
