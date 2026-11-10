use crate::domain::ClientInner;
use crate::pane::mousestate::MouseState;
use crate::pane::renderable::{ask_changes, hydrate_lines, RenderableInner, RenderableState};
use anyhow::bail;
use async_trait::async_trait;
use codec::*;
use config::configuration;
use config::keyassignment::ScrollbackEraseMode;
use mux::domain::DomainId;
use mux::pane::{
    alloc_pane_id, CachePolicy, CloseReason, ForEachPaneLogicalLine, LogicalLine, Pane, PaneId,
    Pattern, SearchResult, WithPaneLines,
};
use mux::renderable::{RenderableDimensions, StableCursorPosition};
use mux::tab::TabId;
use mux::{Mux, MuxNotification};
use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};
use rangeset::RangeSet;
use ratelim::RateLimiter;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use termwiz::hyperlink::Rule;
use termwiz::input::KeyEvent;
use termwiz::surface::SequenceNo;
use url::Url;
use wezterm_dynamic::Value;
use wezterm_term::color::ColorPalette;
use wezterm_term::program_status::ProgramStatusRecord;
use wezterm_term::{
    Alert, Clipboard, KeyCode, KeyModifiers, Line, MouseEvent, Progress, StableRowIndex,
    TerminalConfiguration, TerminalSize,
};

pub struct ClientPane {
    client: Arc<ClientInner>,
    local_pane_id: PaneId,
    pub remote_pane_id: PaneId,
    pub remote_tab_id: TabId,
    pub renderable: Mutex<RenderableState>,
    configured_palette: Mutex<ColorPalette>,
    palette: Mutex<ColorPalette>,
    application_palette: Mutex<bool>,
    writer: Mutex<PaneWriter>,
    mouse: Arc<Mutex<MouseState>>,
    clipboard: Mutex<Option<Arc<dyn Clipboard>>>,
    mouse_grabbed: Mutex<bool>,
    alt_screen_active: Mutex<bool>,
    ignore_next_kill: Mutex<bool>,
    user_vars: Mutex<HashMap<String, String>>,
    config: Mutex<Option<Arc<dyn TerminalConfiguration>>>,
    unseen_output: Mutex<bool>,
    progress: Mutex<Progress>,
    /// LBTerm: the server's OSC 7501 records for this pane
    program_status: Mutex<Vec<ProgramStatusRecord>>,
    /// LBTerm: the Resize requests sent that the server hasn't answered yet (lbterm_sizes.rs)
    lbterm_resizes: super::lbterm_sizes::Unanswered,
    /// LBTerm: the renderable's count of pushes and fetch answers that brought it rows
    /// (`RenderableInner::lbterm_rows`), read without its lock
    lbterm_rows: Arc<AtomicU64>,
    /// LBTerm: the renderable's word from the server that it holds this pane's lines
    /// (`RenderableInner::lbterm_held`), read without its lock
    lbterm_held: Arc<AtomicU64>,
    /// LBTerm: the renderable's count of the holds a push ended (`RenderableInner::lbterm_held`)
    lbterm_holds_ended: Arc<AtomicU64>,
    /// LBTerm: the renderable's newest push applied and whether it's been said read since
    /// (`RenderableInner::lbterm_drawn`), taken without its lock by the window's paint
    lbterm_drawn: Arc<crate::lbterm_drawn::Drawn>,
}

impl ClientPane {
    pub fn new(
        client: &Arc<ClientInner>,
        remote_tab_id: TabId,
        remote_pane_id: PaneId,
        size: TerminalSize,
        title: &str,
    ) -> Self {
        let local_pane_id = alloc_pane_id();
        let writer = PaneWriter {
            client: Arc::clone(client),
            remote_pane_id,
            local_pane_id,
        };

        let mouse = Arc::new(Mutex::new(MouseState::new(
            remote_pane_id,
            client.client.clone(),
        )));

        let fetch_limiter =
            RateLimiter::new(|config| config.ratelimit_mux_line_prefetches_per_second);

        let render = RenderableState {
            inner: RefCell::new(RenderableInner::new(
                client,
                remote_pane_id,
                local_pane_id,
                RenderableDimensions {
                    cols: size.cols as _,
                    viewport_rows: size.rows as _,
                    scrollback_rows: size.rows as _,
                    physical_top: 0,
                    scrollback_top: 0,
                    dpi: size.dpi,
                    pixel_width: size.pixel_width,
                    pixel_height: size.pixel_height,
                    reverse_video: false,
                },
                title,
                fetch_limiter,
            )),
        };

        let lbterm_rows = Arc::clone(&render.inner.borrow().lbterm_rows);
        let lbterm_held = Arc::clone(&render.inner.borrow().lbterm_held);
        let lbterm_holds_ended = Arc::clone(&render.inner.borrow().lbterm_holds_ended);
        let lbterm_drawn = Arc::clone(&render.inner.borrow().lbterm_drawn);

        let config = configuration();
        let palette: ColorPalette = config.resolved_palette.clone().into();

        // Advise the server of our palette preference
        promise::spawn::spawn({
            let palette = palette.clone();
            let client = Arc::clone(client);
            async move {
                client
                    .client
                    .set_configured_palette_for_pane(SetPalette {
                        pane_id: remote_pane_id,
                        palette,
                    })
                    .await
            }
        })
        .detach();

        Self {
            client: Arc::clone(client),
            mouse,
            remote_pane_id,
            local_pane_id,
            remote_tab_id,
            application_palette: Mutex::new(false),
            renderable: Mutex::new(render),
            writer: Mutex::new(writer),
            configured_palette: Mutex::new(palette.clone()),
            palette: Mutex::new(palette),
            clipboard: Mutex::new(None),
            mouse_grabbed: Mutex::new(false),
            alt_screen_active: Mutex::new(false),
            ignore_next_kill: Mutex::new(false),
            unseen_output: Mutex::new(false),
            user_vars: Mutex::new(HashMap::new()),
            config: Mutex::new(None),
            progress: Mutex::new(Progress::default()),
            program_status: Mutex::new(vec![]),
            lbterm_resizes: Default::default(),
            lbterm_rows,
            lbterm_held,
            lbterm_holds_ended,
            lbterm_drawn,
        }
    }

    /// LBTerm: whether a Resize this pane sent hasn't been answered yet, so the server's sizes
    /// listed meanwhile can be older than the one it asked for (lbterm_sizes.rs)
    pub fn lbterm_resize_unanswered(&self) -> bool {
        self.lbterm_resizes.any()
    }

    /// LBTerm: whether this pane's connection is down, so what's sent to it waits for the
    /// reconnect (lbterm_link.rs): the scroll strip doesn't start a seek then
    pub fn lbterm_link_down(&self) -> bool {
        self.client.client.lbterm_link_down()
    }

    /// LBTerm: write `data` to the pane and hear how it came out, for a caller that keeps what
    /// it wrote until it's there: a voice draft, put back over the pane when a key of its send
    /// fails (Lachlan's 3a of 2026-11-08; wezterm-gui's termwindow/control.rs). The same request
    /// as a `PaneWriter`'s, in order with every other this window makes, waiting through a
    /// reconnect as they do (lbterm_link.rs). The caller says a failure itself: nothing is told
    /// to mux::lbterm_writes, so it isn't said twice
    pub fn lbterm_write_answered(
        &self,
        data: &[u8],
    ) -> impl std::future::Future<Output = Option<crate::lbterm_link::Failure>> {
        let client = self.client.client.clone();
        let pdu = WriteToPane {
            pane_id: self.remote_pane_id,
            data: data.to_vec(),
        };
        async move { client.lbterm_write_to_pane(pdu).await }
    }

    /// LBTerm: how many pushes and fetch answers have brought this pane new rows, a line whose
    /// text it didn't hold or a row named and not carried, so far (renderable.rs,
    /// `brings_new_text`): the same count again means the rows it holds read as they did, a push
    /// of its title, folder or cursor alone since. Read without the renderable's lock, so
    /// from the GUI's mux subscription too, which a push's PaneOutput reaches while it's held
    /// (wezterm-gui's hidden_scan.rs)
    pub fn lbterm_rows_changed(&self) -> u64 {
        self.lbterm_rows.load(Ordering::Acquire)
    }

    /// LBTerm: whether the server holds lines of this pane the window hasn't had: it said so
    /// (PaneHeld) and no push past what it said has come. A frame that would show the pane for
    /// the first time waits for that push, up to a bound (wezterm-gui's termwindow/frame_wait.rs;
    /// lbterm/MUX.md, "Tabs out of view: output held at the server"). Read without the
    /// renderable's lock
    pub fn lbterm_is_held(&self) -> bool {
        self.lbterm_held.load(Ordering::Acquire) != 0
    }

    /// LBTerm: what a read that must see the pane as its program has it does first (Lachlan's 6b
    /// of 2026-11-08; lbterm_current.rs): nothing when the pane isn't held, which costs an atomic
    /// load, nor while the connection is down or an ask on it has run out its bound unanswered
    /// (logged at debug); else its push asked of the server, to wait on
    fn lbterm_ask(&self) -> Option<crate::lbterm_current::Ask> {
        use crate::lbterm_current::{plan, Plan};
        let held = self.lbterm_is_held();
        if !held {
            return None;
        }
        let client = &self.client.client;
        match plan(held, client.lbterm_link_down(), client.lbterm_reads().is_late()) {
            Plan::Read(why) => {
                log::debug!(
                    target: "lbterm::shown",
                    "a read of held pane {} (the server's {}) reads it as last pushed: {why:?}",
                    self.local_pane_id,
                    self.remote_pane_id
                );
                None
            }
            Plan::Ask => {
                let Some(answer) = client.lbterm_ask_changes(self.remote_pane_id) else {
                    log::debug!(
                        target: "lbterm::shown",
                        "a read of held pane {}: the client has ended, so nothing to ask",
                        self.local_pane_id
                    );
                    return None;
                };
                Some(crate::lbterm_current::Ask {
                    client: client.clone(),
                    remote_pane_id: self.remote_pane_id,
                    local_pane_id: self.local_pane_id,
                    held: Arc::clone(&self.lbterm_held),
                    answer,
                    asked: std::time::Instant::now(),
                    bound: config::lbterm::HELD_READ_WAIT,
                })
            }
        }
    }

    /// LBTerm: a frame of the window showing this pane read it (wezterm-gui's
    /// termwindow/panes_drawn.rs), or no frame will read what came: the newest push applied, by
    /// the server's id, once a push and only while the server knows PanesDrawn, to say in the
    /// paint's one notice to this pane's server (lbterm_drawn.rs). A frame with nothing new
    /// costs an atomic load
    pub fn lbterm_frame_read(&self) -> Option<codec::PaneDrawn> {
        if !self.lbterm_drawn.unsaid() || !self.client.client.lbterm_says_drawn() {
            return None;
        }
        let seqno = self.lbterm_drawn.take()?;
        Some(codec::PaneDrawn {
            pane_id: self.remote_pane_id,
            seqno,
        })
    }

    /// LBTerm: whether a push applied hasn't been said read yet and this pane's server would
    /// hear it (lbterm_drawn.rs). The gate first: a pane no frame reads keeps `unsaid` set for
    /// good where nothing takes it (a pane out of view, every pane of a server that doesn't know
    /// PanesDrawn), so without it each PaneOutput under the Conductor's view would go on to look
    /// up the active tab and walk it to say nothing. Two atomic loads
    pub fn lbterm_unsaid(&self) -> bool {
        self.client.client.lbterm_says_drawn() && self.lbterm_drawn.unsaid()
    }

    /// LBTerm: `panes`, which frames read, said to this pane's server in one PanesDrawn
    pub fn lbterm_say_drawn(&self, panes: Vec<codec::PaneDrawn>) {
        self.client.client.lbterm_panes_drawn(panes);
    }

    /// LBTerm: the push applied last said read now, for a window showing the pane that won't
    /// draw it soon (lbterm_drawn.rs)
    pub fn lbterm_say_now(&self) {
        if let Some(drawn) = self.lbterm_frame_read() {
            self.lbterm_say_drawn(vec![drawn]);
        }
    }

    /// LBTerm: whether `other` is a pane of the same connection, so one PanesDrawn says both
    pub fn lbterm_same_server(&self, other: &ClientPane) -> bool {
        Arc::ptr_eq(&self.client, &other.client)
    }

    /// LBTerm: how many of the server's holds of this pane a push has ended, counted before the
    /// push's PaneOutput: a push that moves it is the server's quiet push (or the one a poll or
    /// a resize asked), so the pane has been quiet at the server for `HELD_QUIET` and its screen
    /// can be scanned for links at once (wezterm-gui's hidden_scan.rs). Read without the
    /// renderable's lock, so from the GUI's mux subscription too
    pub fn lbterm_holds_ended(&self) -> u64 {
        self.lbterm_holds_ended.load(Ordering::Acquire)
    }

    /// LBTerm: the domain has reconnected and its panes are mapped again (domain.rs's
    /// `reattach`): a pane the old connection said it holds stays held, from no push, and its
    /// push is asked of the new connection, which owes nothing for a pane quiet since and would
    /// never send it, so a switch to a session that finished out of view waits for its screen
    /// as the server has it rather than drawing what came before the hold (PITFALLS 201)
    pub(crate) fn lbterm_reconnected(&self) {
        if super::renderable::lbterm_hold_reconnected(&self.lbterm_held) {
            // At info: once a held pane a reconnect, and perf/mux-hidden.ps1's step 8 reads it to
            // know the hold outlived the connection that said it
            log::info!(
                target: "lbterm::shown",
                "pane {} (the server's {}) was held: its push asked of the new connection",
                self.local_pane_id,
                self.remote_pane_id
            );
            promise::spawn::spawn(ask_changes(
                Arc::clone(&self.client),
                self.remote_pane_id,
                self.local_pane_id,
                false,
            ))
            .detach();
        }
    }

    pub async fn process_unilateral(&self, pdu: Pdu) -> anyhow::Result<()> {
        match pdu {
            Pdu::GetPaneRenderChangesResponse(mut delta) => {
                // LBTerm: either changing is something the GUI draws (renderable.rs)
                let flags_changed = *self.mouse_grabbed.lock() != delta.mouse_grabbed
                    || *self.alt_screen_active.lock() != delta.alt_screen_active;
                *self.mouse_grabbed.lock() = delta.mouse_grabbed;
                *self.alt_screen_active.lock() = delta.alt_screen_active;

                let bonus_lines = std::mem::take(&mut delta.bonus_lines);
                let client = { Arc::clone(&self.renderable.lock().inner.borrow().client) };
                let bonus_lines = hydrate_lines(client, delta.pane_id, bonus_lines).await;

                self.renderable
                    .lock()
                    .inner
                    .borrow_mut()
                    .apply_changes_to_surface(delta, bonus_lines, flags_changed);
            }
            Pdu::SetClipboard(SetClipboard {
                clipboard,
                selection,
                ..
            }) => match self.clipboard.lock().as_ref() {
                Some(clip) => {
                    log::debug!(
                        "Pdu::SetClipboard pane={} remote={} {:?} {:?}",
                        self.local_pane_id,
                        self.remote_pane_id,
                        selection,
                        clipboard
                    );
                    clip.set_contents(selection, clipboard)?;
                }
                None => {
                    log::error!("ClientPane: Ignoring SetClipboard request {:?}", clipboard);
                }
            },
            Pdu::SetPalette(SetPalette { palette, .. }) => {
                *self.application_palette.lock() = palette != *self.configured_palette.lock();

                // LBTerm: the palette it has already changes nothing. A config save sent the
                // server every pane's palette, which it sent back, and again once it had read
                // the same file itself: each made every line of the pane stale, to be fetched
                // again, and threw away the window's shapes (lbterm/MUX.md, "What a config save
                // costs"). A server from before this still sends them
                if *self.palette.lock() == palette {
                    log::debug!(target: "lbterm::palette", "palette pane {}: the one it has", self.local_pane_id);
                    return Ok(());
                }
                *self.palette.lock() = palette;
                let mux = Mux::get();
                self.renderable.lock().inner.borrow_mut().make_all_stale();
                mux.notify(MuxNotification::Alert {
                    pane_id: self.local_pane_id,
                    alert: Alert::PaletteChanged,
                });
            }
            Pdu::SetProgramStatus(SetProgramStatus { records, .. }) => {
                *self.program_status.lock() = records;
                let mux = Mux::get();
                mux.notify(MuxNotification::Alert {
                    pane_id: self.local_pane_id,
                    alert: Alert::ProgramStatus,
                });
            }
            Pdu::NotifyAlert(NotifyAlert { alert, .. }) => {
                let mux = Mux::get();
                // LBTerm: each alert is passed on once, below; these two were passed on here as
                // well, so every window updated its title twice for each
                match &alert {
                    Alert::SetUserVar { name, value } => {
                        self.user_vars.lock().insert(name.clone(), value.clone());
                    }
                    Alert::OutputSinceFocusLost => {
                        *self.unseen_output.lock() = true;
                    }
                    Alert::Progress(progress) => {
                        *self.progress.lock() = progress.clone();
                    }
                    _ => {}
                }
                mux.notify(MuxNotification::Alert {
                    pane_id: self.local_pane_id,
                    alert,
                });
            }
            Pdu::PaneRemoved(PaneRemoved { pane_id }) => {
                log::trace!("remote pane {} has been removed", pane_id);
                self.renderable.lock().inner.borrow_mut().dead = true;
                let mux = Mux::get();
                mux.prune_dead_windows();

                self.client.expire_stale_mappings();
            }
            // LBTerm: the server holds this pane's lines from its push of `seqno` while no window
            // shows it (lbterm/MUX.md, "Tabs out of view: output held at the server"). A server
            // sends it only to a window that has said what it shows (lbterm_shown.rs)
            Pdu::PaneHeld(PaneHeld { seqno, .. }) => {
                let held = self
                    .renderable
                    .lock()
                    .inner
                    .borrow_mut()
                    .lbterm_held_from(seqno);
                log::debug!(
                    target: "lbterm::shown",
                    "pane {} (server's {}): held from seqno {seqno}{}",
                    self.local_pane_id,
                    self.remote_pane_id,
                    if held { "" } else { ", a push past it already applied" }
                );
            }
            Pdu::PaneFocused(PaneFocused { pane_id }) => {
                // We get here whenever the pane focus is changed on the
                // server. That might be due to the user here in the GUI
                // doing things, or it may be due to a "remote"
                // `wezterm cli activate-pane-direction` or similar call
                // from some other actor.
                // The latter case is the important one: it is desirable
                // for the focus change to be reflected locally after it
                // has been changed on the server, so we work to apply
                // it here.
                log::trace!("advised of remote pane focus: {pane_id}");

                let mux = Mux::get();
                if let Err(err) = mux.focus_pane_and_containing_tab(self.local_pane_id) {
                    log::error!("Error reconciling remote PaneFocused notification: {err:#}");
                }
            }
            _ => bail!("unhandled unilateral pdu: {:?}", pdu),
        };
        Ok(())
    }

    pub fn remote_pane_id(&self) -> TabId {
        self.remote_pane_id
    }

    /// LBTerm: `push`, of the pane's title and folder alone, applied here on the client thread
    /// as it's read, when it changes nothing else the pane holds (`RenderableInner::lbterm_names_here`,
    /// lbterm_names.rs). Whether it was. The flags compared here are set only as a push of the
    /// pane is applied, and none of its waits (`Backlog::waits`), so none is applied meanwhile
    pub(crate) fn lbterm_names_here(&self, push: &GetPaneRenderChangesResponse) -> bool {
        let flags_same = *self.mouse_grabbed.lock() == push.mouse_grabbed
            && *self.alt_screen_active.lock() == push.alt_screen_active;
        self.renderable
            .lock()
            .inner
            .borrow_mut()
            .lbterm_names_here(push, flags_same)
    }

    /// LBTerm: `rows` lent to `with` where this pane holds them, read as get_lines reads them,
    /// so what `with` leaves on a line (the key the window caches its shapes by) stays with it
    /// (lbterm_lent.rs). `with` must not reach this pane: it's held for the call. wezterm-gui's
    /// render/pane_lines.rs copies a frame's rows through it
    pub fn lbterm_with_lines_lent(&self, rows: Range<StableRowIndex>, with: &mut dyn WithPaneLines) {
        self.renderable.lock().lbterm_with_lines_lent(rows, with);
    }

    /// Arrange to suppress the next Pane::kill call.
    /// This is a bit of a hack that we use when closing a window;
    /// our Domain::local_window_is_closing impl calls this for each
    /// ClientPane in the window so that closing a window effectively
    /// "detaches" the window so that reconnecting later will resume
    /// from where they left off.
    /// It isn't perfect.
    pub fn ignore_next_kill(&self) {
        *self.ignore_next_kill.lock() = true;
    }
}

#[async_trait(?Send)]
impl Pane for ClientPane {
    fn pane_id(&self) -> PaneId {
        self.local_pane_id
    }

    fn get_metadata(&self) -> Value {
        let renderable = self.renderable.lock();
        let inner = renderable.inner.borrow();

        let mut map: BTreeMap<Value, Value> = BTreeMap::new();
        map.insert(
            Value::String("is_tardy".to_string()),
            Value::Bool(inner.is_tardy()),
        );
        map.insert(
            Value::String("since_last_response_ms".to_string()),
            Value::U64(inner.last_recv_time.elapsed().as_millis() as u64),
        );
        // LBTerm: the pane's id in the mux server, which is what WEZTERM_PANE
        // says inside it and what `lbterm cli` there takes. pane_id() is
        // this GUI's own number for it.
        map.insert(
            Value::String("remote_pane_id".to_string()),
            Value::U64(self.remote_pane_id as u64),
        );

        Value::Object(map.into())
    }

    fn get_cursor_position(&self) -> StableCursorPosition {
        self.renderable.lock().get_cursor_position()
    }

    fn get_dimensions(&self) -> RenderableDimensions {
        self.renderable.lock().get_dimensions()
    }

    /// LBTerm: the pane's push asked of its server while it holds the pane, for a reader in a
    /// task to await before it reads (lbterm_current.rs)
    fn lbterm_current(&self) -> Option<std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>> {
        let ask = self.lbterm_ask()?;
        Some(Box::pin(ask.wait()))
    }

    /// LBTerm: the same, waited for on this thread (Lua's reads, on the GUI thread), up to
    /// config's `HELD_READ_WAIT`: the client thread fulfils the answer, so nothing the GUI thread
    /// would run is needed for it, and the pane's pushes are applied here (lbterm_current.rs)
    fn lbterm_current_now(&self) {
        if let Some(ask) = self.lbterm_ask() {
            smol::block_on(ask.wait());
        }
    }

    fn with_lines_mut(&self, lines: Range<StableRowIndex>, with_lines: &mut dyn WithPaneLines) {
        mux::pane::impl_with_lines_via_get_lines(self, lines, with_lines);
    }

    fn for_each_logical_line_in_stable_range_mut(
        &self,
        lines: Range<StableRowIndex>,
        for_line: &mut dyn ForEachPaneLogicalLine,
    ) {
        mux::pane::impl_for_each_logical_line_via_get_logical_lines(self, lines, for_line);
    }

    fn get_lines(&self, lines: Range<StableRowIndex>) -> (StableRowIndex, Vec<Line>) {
        self.renderable.lock().get_lines(lines)
    }

    fn get_logical_lines(&self, lines: Range<StableRowIndex>) -> Vec<LogicalLine> {
        mux::pane::impl_get_logical_lines_via_get_lines(self, lines)
    }

    /// LBTerm: the lines held for `lines` scanned for implicit hyperlinks in place, once each,
    /// as they're drawn or looked at for a link: upstream scanned every line as it came
    /// (renderable.rs, scan_rows), and its default here scanned copies it threw away
    fn apply_hyperlinks(&self, lines: Range<StableRowIndex>, rules: &[Rule]) {
        self.renderable
            .lock()
            .inner
            .borrow_mut()
            .apply_hyperlinks(lines, rules);
    }

    fn get_current_seqno(&self) -> SequenceNo {
        self.renderable.lock().get_current_seqno()
    }

    fn get_changed_since(
        &self,
        lines: Range<StableRowIndex>,
        seqno: SequenceNo,
    ) -> RangeSet<StableRowIndex> {
        self.renderable.lock().get_changed_since(lines, seqno)
    }

    fn set_clipboard(&self, clipboard: &Arc<dyn Clipboard>) {
        self.clipboard.lock().replace(Arc::clone(clipboard));
    }

    fn get_title(&self) -> String {
        let renderable = self.renderable.lock();
        let inner = renderable.inner.borrow();
        inner.title.clone()
    }

    fn get_program_status(&self) -> Vec<ProgramStatusRecord> {
        self.program_status.lock().clone()
    }

    fn get_progress(&self) -> Progress {
        self.progress.lock().clone()
    }

    fn send_paste(&self, text: &str) -> anyhow::Result<()> {
        let client = Arc::clone(&self.client);
        let remote_pane_id = self.remote_pane_id;
        self.renderable
            .lock()
            .inner
            .borrow_mut()
            .predict_from_paste(text);

        let data = text.to_owned();
        promise::spawn::spawn(async move {
            client
                .client
                .send_paste(SendPaste {
                    pane_id: remote_pane_id,
                    data,
                })
                .await
        })
        .detach();
        self.renderable.lock().inner.borrow_mut().update_last_send();
        Ok(())
    }

    fn reader(&self) -> anyhow::Result<Option<Box<dyn std::io::Read + Send>>> {
        Ok(None)
    }

    fn writer(&self) -> MappedMutexGuard<'_, dyn std::io::Write> {
        MutexGuard::map(self.writer.lock(), |writer| {
            let w: &mut dyn std::io::Write = writer;
            w
        })
    }

    fn set_zoomed(&self, zoomed: bool) {
        let render = self.renderable.lock();
        let mut inner = render.inner.borrow_mut();
        let client = Arc::clone(&self.client);
        let remote_pane_id = self.remote_pane_id;
        let remote_tab_id = self.remote_tab_id;
        // LBTerm: no row made stale, and no poll brought back. A zoom changes the pane's size,
        // which the tab gives it straight after this (mux's TabInner::toggle_zoom), and its
        // resize asks for the push that says what the server's rewrap changed (resize, below).
        // Upstream made every row stale here, and resize again, so the frame after a zoom
        // fetched the rows in view that the push brings too
        promise::spawn::spawn(async move {
            client
                .client
                .set_zoomed(SetPaneZoomed {
                    containing_tab_id: remote_tab_id,
                    pane_id: remote_pane_id,
                    zoomed,
                })
                .await
        })
        .detach();
        inner.update_last_send();
    }

    fn resize(&self, size: TerminalSize) -> anyhow::Result<()> {
        let render = self.renderable.lock();
        let mut inner = render.inner.borrow_mut();

        let cols = size.cols as usize;
        let rows = size.rows as usize;

        if inner.dimensions.cols != cols
            || inner.dimensions.viewport_rows != rows
            || inner.dimensions.pixel_width != size.pixel_width
            || inner.dimensions.pixel_height != size.pixel_height
        {
            inner.dimensions.cols = cols;
            inner.dimensions.viewport_rows = rows;
            inner.dimensions.pixel_width = size.pixel_width;
            inner.dimensions.pixel_height = size.pixel_height;

            // LBTerm: the rows held stay as they are, and the server's word is taken for which
            // of them the resize changed: once the server has rewrapped the pane, the push asked
            // for below names every row its rewrap joined, cut or moved, or ConPTY's redraw
            // wrote, with the lines of those in view (term's lbterm_rewrap, PITFALLS 142), and a
            // row it doesn't name shows what it did. Upstream made every row stale here, so the
            // frame after each resize fetched the rows in view again, though the push brought
            // them: closing a pane beside another drew a frame more than the installed build
            // (budgets' close-pane, 3 frames to 4 in 67 of 75 closes, a GetLines more) once this
            // branch's server stopped resizing a pane again to the size it had (lbterm/MUX.md,
            // "A window's resize"; FLOWS.md's resize flow, rows 4 and 13). A size that came from
            // the server (a resync's) isn't sent back to it (lbterm_sizes.rs)
            let from_server = super::lbterm_sizes::is_from_server();
            log::debug!(
                target: "lbterm::resize",
                "pane {} (the server's {}) to {}x{}{}",
                self.local_pane_id,
                self.remote_pane_id,
                cols,
                rows,
                if from_server { ", the server's size" } else { ": sent" }
            );
            let client = Arc::clone(&self.client);
            let remote_pane_id = self.remote_pane_id;
            let local_pane_id = self.local_pane_id;
            if !from_server {
                let remote_tab_id = self.remote_tab_id;
                // LBTerm: unanswered until its answer comes, whatever it says (lbterm_sizes.rs)
                let sent = self.lbterm_resizes.sent();
                promise::spawn::spawn(async move {
                    let answer = client
                        .client
                        .resize(Resize {
                            containing_tab_id: remote_tab_id,
                            pane_id: remote_pane_id,
                            size,
                        })
                        .await;
                    // LBTerm: the server answers a resize once it has rewrapped the pane, with no
                    // push, and a quiet pane's ConPTY may write nothing: the push that says what
                    // the rewrap changed is asked for now, of a server of any build (one before
                    // this answers in order on its main thread), for a pane in view or not
                    // (PITFALLS 167 and 173). Only by the last Resize answered: one sent after
                    // this and still out asks once it's answered, so a drag whose answers come
                    // behind its next sizes asks at its end, not at each size (FLOWS.md's resize
                    // flow, What could go 9); a pane in view is polled with its frames meanwhile
                    if sent.answered() {
                        ask_changes(client, remote_pane_id, local_pane_id, false).await?;
                    }
                    answer.map(|_| ())
                })
                .detach();
                inner.update_last_send();
            } else if !self.lbterm_resizes.any() {
                // LBTerm: the server's own size, which it has rewrapped the pane to before it
                // listed it: its push is asked for, as above. A Resize of this window's still
                // out asks for its own once it's answered
                promise::spawn::spawn(ask_changes(client, remote_pane_id, local_pane_id, false))
                    .detach();
            }
        }
        Ok(())
    }

    async fn search(
        &self,
        pattern: Pattern,
        range: Range<StableRowIndex>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<SearchResult>> {
        match self
            .client
            .client
            .search_scrollback(SearchScrollbackRequest {
                pane_id: self.remote_pane_id,
                pattern,
                range,
                limit,
            })
            .await
        {
            Ok(SearchScrollbackResponse { results }) => Ok(results),
            Err(e) => Err(e),
        }
    }

    fn key_down(&self, key: KeyCode, mods: KeyModifiers) -> anyhow::Result<()> {
        let input_serial;
        {
            let renderable = self.renderable.lock();
            let mut inner = renderable.inner.borrow_mut();
            inner.input_serial = InputSerial::now();
            input_serial = inner.input_serial;
            inner.predict_from_key_event(key, mods);
        }
        let client = Arc::clone(&self.client);
        let remote_pane_id = self.remote_pane_id;
        promise::spawn::spawn(async move {
            client
                .client
                .key_down(SendKeyDown {
                    pane_id: remote_pane_id,
                    event: KeyEvent {
                        key,
                        modifiers: mods,
                    },
                    input_serial,
                })
                .await
        })
        .detach();
        self.renderable.lock().inner.borrow_mut().update_last_send();
        Ok(())
    }

    fn key_up(&self, _key: KeyCode, _mods: KeyModifiers) -> anyhow::Result<()> {
        // TODO: decide how to handle key_up for mux client
        Ok(())
    }

    fn kill(&self) {
        let mut ignore = self.ignore_next_kill.lock();
        if *ignore {
            *ignore = false;
            return;
        }
        let client = Arc::clone(&self.client);
        let remote_pane_id = self.remote_pane_id;
        let local_domain_id = self.client.local_domain_id;

        // We only want to ask the server to kill the pane if the user
        // explicitly requested it to die.
        // Domain detaching can implicitly call Pane::kill on the panes
        // in the domain, so we need to check here whether the domain is
        // in the detached state; if so then we must skip sending the
        // kill to the server.
        let mut send_kill = true;

        {
            let mux = Mux::get();
            if let Some(client_domain) = mux.get_domain(local_domain_id) {
                if client_domain.state() == mux::domain::DomainState::Detached {
                    send_kill = false;
                }
            }
        }

        if send_kill {
            // LBTerm: closing a window kills its panes here rather than
            // detaching (Mux::remove_window_internal), and the GUI exits
            // once the mux is empty and no Activity is held. Hold one until
            // the server has the kill, or the last window's panes would live
            // on in the server. Bounded, so a server that stopped answering
            // can't keep a closed LBTerm running: the bound lets go of the
            // Activity, never of the request. The KillPane waits for its
            // answer however long it takes (a reconnect, the server's
            // Resizes::after_all), since an answer that finds its receiver
            // dropped makes the client thread take itself for destroyed and
            // detach the domain, every tab leaving the window
            // (lbterm/PITFALLS.md, 171)
            let activity = std::rc::Rc::new(std::cell::Cell::new(Some(mux::activity::Activity::new())));
            let bound = std::rc::Rc::clone(&activity);
            promise::spawn::spawn(async move {
                smol::Timer::after(std::time::Duration::from_secs(3)).await;
                if bound.take().is_some() {
                    log::warn!("the server hasn't answered the kill of pane {remote_pane_id} in 3 s; still waiting for it");
                }
            })
            .detach();
            promise::spawn::spawn(async move {
                let result = client
                    .client
                    .kill_pane(KillPane {
                        pane_id: remote_pane_id,
                    })
                    .await;
                activity.take();
                if let Err(err) = &result {
                    log::error!("killing remote pane {remote_pane_id}: {err:#}");
                }
                result
            })
            .detach();
        }
    }

    fn mouse_event(&self, event: MouseEvent) -> anyhow::Result<()> {
        // LBTerm: a move the server would only drop isn't sent (lbterm_mouse.rs)
        if !super::lbterm_mouse::sends(&event, self.is_mouse_grabbed()) {
            return Ok(());
        }
        self.mouse.lock().append(event);
        if MouseState::next(Arc::clone(&self.mouse)) {
            self.renderable.lock().inner.borrow_mut().update_last_send();
        }
        Ok(())
    }

    fn is_dead(&self) -> bool {
        self.renderable.lock().inner.borrow().dead
    }

    fn palette(&self) -> ColorPalette {
        self.palette.lock().clone()
    }

    fn domain_id(&self) -> DomainId {
        self.client.local_domain_id
    }

    fn is_mouse_grabbed(&self) -> bool {
        *self.mouse_grabbed.lock()
    }

    fn is_alt_screen_active(&self) -> bool {
        *self.alt_screen_active.lock()
    }

    fn get_current_working_dir(&self, _policy: CachePolicy) -> Option<Url> {
        self.renderable.lock().inner.borrow().working_dir.clone()
    }

    fn focus_changed(&self, focused: bool) {
        if focused {
            self.advise_focus();
            *self.unseen_output.lock() = false;
        }
    }

    fn erase_scrollback(&self, erase_mode: ScrollbackEraseMode) {
        let client = Arc::clone(&self.client);
        let remote_pane_id = self.remote_pane_id;
        promise::spawn::spawn(async move {
            client
                .client
                .erase_scrollback(EraseScrollbackRequest {
                    pane_id: remote_pane_id,
                    erase_mode,
                })
                .await
        })
        .detach();
    }

    fn advise_focus(&self) {
        let mut focused_pane = self.client.focused_remote_pane_id.lock().unwrap();
        if *focused_pane != Some(self.remote_pane_id) {
            focused_pane.replace(self.remote_pane_id);
            let client = Arc::clone(&self.client);
            let remote_pane_id = self.remote_pane_id;
            promise::spawn::spawn(async move {
                client
                    .client
                    .set_focused_pane_id(SetFocusedPane {
                        pane_id: remote_pane_id,
                    })
                    .await
            })
            .detach();
        }
    }

    fn has_unseen_output(&self) -> bool {
        *self.unseen_output.lock()
    }

    fn can_close_without_prompting(&self, reason: CloseReason) -> bool {
        match reason {
            CloseReason::Window => true,
            CloseReason::Tab => false,
            CloseReason::Pane => false,
        }
    }

    fn copy_user_vars(&self) -> HashMap<String, String> {
        self.user_vars.lock().clone()
    }

    fn set_config(&self, config: Arc<dyn TerminalConfiguration>) {
        let palette = config.color_palette();
        // If the application running in the pane hasn't changed the
        // palette through escape sequences, speculatively adopt the
        // new palette so that it updates with the lowest latency.
        if !*self.application_palette.lock() {
            *self.palette.lock() = palette.clone();
        }
        // LBTerm: the server has this one already (ClientPane::new sends it, and so did each
        // change): every config save and every window's reload sent each pane's palette again
        let sent = std::mem::replace(&mut *self.configured_palette.lock(), palette.clone()) == palette;
        self.config.lock().replace(config);
        if sent {
            return;
        }

        // and now send the color palette to the server
        let client = Arc::clone(&self.client);
        let remote_pane_id = self.remote_pane_id;
        promise::spawn::spawn(async move {
            client
                .client
                .set_configured_palette_for_pane(SetPalette {
                    pane_id: remote_pane_id,
                    palette,
                })
                .await
        })
        .detach();
    }

    fn get_config(&self) -> Option<Arc<dyn TerminalConfiguration>> {
        self.config.lock().clone()
    }
}

struct PaneWriter {
    client: Arc<ClientInner>,
    remote_pane_id: TabId,
    /// LBTerm: the window's id for the pane, which a failure is told under
    local_pane_id: PaneId,
}

impl std::io::Write for PaneWriter {
    /// LBTerm: sent from a task, as a key is (key_down) and a paste, and not waited for. Upstream
    /// waited here for the server's answer (block_on), which it gives once it has written the
    /// bytes to the pane's program and pushed the pane, behind whatever its only thread is doing
    /// for every other pane: every write a window makes to a mux pane held its GUI thread that
    /// long (a key binding's SendString, Lachlan's Shift+Enter and Alt+Enter, smart paste's
    /// image, the scroll strip's keys, a voice draft, an IME's or the char selector's text, a key
    /// sent in win32-input or kitty mode, Lua's pane:send_text). And it went ahead of every key
    /// still waiting in a task to be sent, so a binding pressed straight after a letter could
    /// reach the program first: in a task it keeps its place behind them. While the connection
    /// is down it waits for the next with the keys round it, never failing for that alone, so
    /// what Lachlan typed reaches the program whole (lbterm_link.rs). A write that fails is
    /// logged and shown on the pane's scroll strip (mux::lbterm_writes), as the task hears it:
    /// the server refused it, the connection ended before it answered, or the client gave up
    /// reconnecting before it went
    fn write(&mut self, data: &[u8]) -> Result<usize, std::io::Error> {
        let (remote, local) = (self.remote_pane_id, self.local_pane_id);
        let client = self.client.client.clone();
        let pdu = WriteToPane {
            pane_id: remote,
            data: data.to_vec(),
        };
        promise::spawn::spawn_into_main_thread(async move {
            if let Some(failure) = client.lbterm_write_to_pane(pdu).await {
                log::error!("writing to remote pane {}: {:?}", remote, failure);
                mux::lbterm_writes::failed(local, failure.to_string(), failure.reached());
            }
        })
        .detach();
        Ok(data.len())
    }

    fn flush(&mut self) -> Result<(), std::io::Error> {
        Ok(())
    }
}
