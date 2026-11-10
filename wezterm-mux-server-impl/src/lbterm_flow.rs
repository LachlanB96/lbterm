//! LBTerm: what a connection owes its client, and what it has for the socket.
//!
//! Upstream turned every PaneOutput and Alert notification into a task of its own that worked out
//! the pane's changes there and then and queued them, encoded later, in the connection's unbounded
//! channel, behind which the connection's writes awaited the client. A client that couldn't keep
//! up (a window whose GUI thread fell behind, or a server whose own thread did) left the changes
//! queuing without bound: on 2026-10-29 six tabs printing without a pause grew a test server to
//! 29.5 GB in about seven minutes, and every key waited behind the queue.
//!
//! A pane's push describes its state, so one not yet sent that a newer would replace needn't be
//! sent at all. [`Pending`] holds which panes have changes owed and the alerts they've raised, each
//! pane once, filled by the connection's mux subscriber as the notifications come; the connection
//! works a pane's push out only when its [`Outbox`] has room, so the push covers everything that
//! changed up to then, and what it holds is bounded by the panes there are. A mux window's title,
//! which follows whichever of its panes set one last, is held the same way, its latest. The pane
//! the client last gave input to is pushed first, so its echo never waits behind the others. The
//! outbox is written as the socket takes it, so the connection reads its client's keys while the
//! client is slow to take its pushes.
//!
//! The connection is told once that something is owed, until it next waits ([`Pending::wait`]),
//! so it must never wait while it still owes something the outbox has room for: nothing would
//! tell it again. A round of pushes stops once the outbox reaches [`OUTBOX_LOW`], and a socket
//! with room takes the lot at once, leaving the outbox empty, nothing for the socket to say and
//! the rest owed (2026-10-31: six hidden tabs printing a screen at the same moment and going
//! quiet left one to three of them unsent, until something else reached the connection).
//!
//! A client that says which panes its windows show (codec's SetShownPanes) has the lines of every
//! other pane it has been pushed held here (lbterm/MUX.md, "Tabs out of view: output held at the
//! server"): such a pane's output is noted in [`Pending`]'s holds with when it came, not owed, so
//! no push of it is worked out; its alerts are owed as before, and a title or folder alert owes a
//! push of them alone ([`Owed::names`]); the first output of a hold owes the client word of it
//! ([`Owed::held`], codec's PaneHeld). A hold ends in a push of everything since, owed like any
//! other: when the client shows the pane (at the front of the order), when it has printed nothing
//! for [`config::lbterm::HELD_QUIET`], or when any push carries its lines (a poll's, a key's;
//! [`Pending::current`]). A client that never says (an older window, the cli) holds nothing.
//!
//! A client that says which push of each shown pane its frames have read (codec's PanesDrawn)
//! has its shown panes paced (lbterm/MUX.md, "A shown pane's pace"; FLOWS.md, pane output, What
//! could go 12): a round's push of a shown pane's changes leaves the pane unread until the client
//! says a frame read that push, and meanwhile [`Pending::take_next`] passes over it, its output
//! owed in the same owe, so the push that goes after the word covers everything since and one
//! push travels while a frame draws. An owe of alerts alone isn't passed over, nor is a pane
//! given input until a frame has read a push made after it, so an echo waits for no frame; and
//! the pushes outside Pending (a key's answer, a poll's) are neither held nor counted. What's
//! passed over rings nothing and counts as owed nowhere (PITFALLS 228): the word that ends it
//! comes as the client's PDU, which the connection's loop reads and goes round after.

use codec::{DecodedPdu, Pdu};
use mux::pane::PaneId;
use mux::window::WindowId;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::ErrorKind;
use std::sync::Mutex;
use std::time::{Duration, Instant};
pub use mux::lbterm_output::note_alert;
use termwiz::surface::SequenceNo;
use wezterm_term::Alert;

/// How many encoded bytes the outbox holds, not yet taken by the socket, below which the
/// connection works out more of the pushes it owes. Above it the client is behind: what changes
/// meanwhile waits in [`Pending`], one push a pane, for the socket to take what's there, so a
/// key's answer and echo wait behind at most this and the socket's [`SEND_BUFFER`].
pub const OUTBOX_LOW: usize = 2 * 1024;

/// The send buffer a local client's socket is given. Windows' AF_UNIX default is 64 KB (a writer
/// with nobody reading got 65,536 bytes in before it would block, 2026-10-30), and a flooding
/// tab's push is about 320 bytes (six such tabs, 2026-10-31: 21,546 pushes in 6.97 MB), so the
/// default held two hundred of them ahead of a key's answer and its echo, each for the window to
/// read and take in turn, and 16 KB still held fifty: the answer came 32 ms after the key at the
/// median, the server having had it in 0.1 ms. A local socket needs no more for its speed: it's a
/// copy in memory, taken as fast as the client reads, and a large answer (a page of scrollback)
/// goes on as the client takes it.
pub const SEND_BUFFER: usize = 4 * 1024;

/// Gives a local client's socket [`SEND_BUFFER`]
pub fn set_send_buffer(stream: &wezterm_uds::UnixStream) -> std::io::Result<()> {
    let size = SEND_BUFFER as i32;
    #[cfg(windows)]
    let res = unsafe {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{setsockopt, SOL_SOCKET, SO_SNDBUF};
        setsockopt(
            stream.as_raw_socket() as _,
            SOL_SOCKET,
            SO_SNDBUF,
            &size as *const i32 as *const _,
            std::mem::size_of::<i32>() as i32,
        )
    };
    #[cfg(unix)]
    let res = unsafe {
        use std::os::unix::io::AsRawFd;
        libc::setsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            &size as *const i32 as *const _,
            std::mem::size_of::<i32>() as libc::socklen_t,
        )
    };
    if res == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// What a pane is owed: the alerts it raised since its last push, and whether its changes are
/// to be worked out and sent with them
#[derive(Default, Debug, Clone, PartialEq)]
pub struct Owed {
    pub alerts: Vec<Alert>,
    /// It printed, or raised an alert whose receiver reads what the push carries (a title or the
    /// cwd, mux::lbterm_output::reads_the_push). An owe of other alerts alone sends them without
    /// the pane read: whatever changed with them came as output, whose own owe carries it
    pub changes: bool,
    /// Its title or folder changed while the client doesn't show it: a push of them alone goes
    /// ahead of its alerts, with no lines and the rest as last sent, since the client reads them
    /// from its last push (PITFALLS 163). Nothing when `changes` is owed too, whose push has them
    pub names: bool,
    /// Its lines are held from now: the client is told so (codec's PaneHeld), once a hold, when
    /// the pane printed since its last push. Nothing when `changes` is owed too
    pub held: bool,
}

/// The panes with changes owed to the client, in the order they came, and what each is owed;
/// and the mux windows whose title changed, each its latest
#[derive(Default, Debug)]
pub struct Pending {
    order: VecDeque<PaneId>,
    owed: HashMap<PaneId, Owed>,
    /// A mux window's title is whichever of its panes set one last, so a tab printing its title
    /// as it goes changes it as often: upstream wrote each to the socket as it came
    titles: BTreeMap<WindowId, String>,
    /// Whether the connection has been told there's something here and hasn't waited since
    rung: bool,
    /// The panes the client's windows show, as it last said (codec's SetShownPanes). None until
    /// it says, and for a client that never does (an older window, the cli), which is pushed
    /// every pane as it changes (lbterm/MUX.md, "Tabs out of view: output held at the server").
    /// Each pane's `drawn` isn't kept: a frame's word comes in PanesDrawn ([`Pending::drawn`])
    shown: Option<HashSet<PaneId>>,
    /// The panes whose lines a push has carried to the client ([`Pending::current`]): only those
    /// are held, since a pane's first push carries the state it already has (its palette, user
    /// vars and records) and the client has nothing of it before
    pushed: HashSet<PaneId>,
    /// The panes out of the client's view whose lines are held, each with when it last printed:
    /// its push goes once it's shown, or quiet for [`config::lbterm::HELD_QUIET`]
    held: HashMap<PaneId, Instant>,
    /// Whether the client says which push of each shown pane its frames have read (codec's
    /// PanesDrawn): set at the first word, after which its shown panes are paced. A client that
    /// never says (an older window, the cli) is pushed every change as it comes, paced only by
    /// its socket (lbterm/MUX.md, "A shown pane's pace")
    paced: bool,
    /// The shown panes a round has pushed whose push no frame has been said to read, each with
    /// that push's seqno and when it went: its next round push waits for the word
    unread: HashMap<PaneId, (SequenceNo, Instant)>,
    /// The panes the client gave input to, each with the seqno of the first round push of it
    /// made since (None until one goes), until a frame has read that push: under a flood the
    /// pushes go on meanwhile, and a word for the latest would never come (perf/mux-pace.ps1,
    /// 2026-11-09: one key early in a flood left the pane unpaced to its end, when the word had
    /// to name a push at least as new as the last one sent). None of their round pushes waits
    /// meanwhile, so an echo waits for no frame
    input: HashMap<PaneId, Option<SequenceNo>>,
    /// Whether the client has asked which tabs are parked (codec's GetParkedTabs), and so knows
    /// codec's TabParked: only then is it told of each park and unpark (PITFALLS 188)
    parked: bool,
}

/// What a connection waits for once it has written what its socket takes now ([`Pending::wait`])
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// Nothing: it owes pushes or titles and its outbox has room for them, so it goes round again
    /// once other tasks have had a turn
    Now,
    /// Room in the socket for what its outbox holds, or its client, or news
    Writable,
    /// Its client or news: it owes nothing, or nothing its outbox has room for and holds nothing
    News,
}

impl Pending {
    /// Mux window `window_id`'s title is now `title`, sent before the next push in place of any
    /// title of it not sent yet. Whether the connection must be told
    pub fn window_title(&mut self, window_id: WindowId, title: String) -> bool {
        self.titles.insert(window_id, title);
        self.ring()
    }

    /// The mux windows' titles not sent yet, each its latest
    pub fn take_titles(&mut self) -> Vec<(WindowId, String)> {
        std::mem::take(&mut self.titles).into_iter().collect()
    }

    /// `pane_id` has new output. Whether the connection must be told (it hasn't been since it
    /// last waited)
    pub fn output(&mut self, pane_id: PaneId) -> bool {
        self.output_at(pane_id, Instant::now())
    }

    /// `pane_id` has new output, at `now`: owed a push, or, out of the client's view, held. A
    /// hold's first output owes the client word of it (PaneHeld) and tells the connection; the
    /// rest only move its quiet deadline on, which the connection reads as it next waits, so a
    /// pane printing out of view wakes it no more (lbterm/FLOWS.md, the pane-output flow, row 21)
    pub fn output_at(&mut self, pane_id: PaneId, now: Instant) -> bool {
        if !self.owed.get(&pane_id).map_or(false, |owed| owed.changes) && self.hides(pane_id) {
            if self.held.insert(pane_id, now).is_some() {
                return false;
            }
            self.owe(pane_id).held = true;
            return self.ring();
        }
        self.owe(pane_id).changes = true;
        self.ring_for(pane_id)
    }

    /// `pane_id` raised `alert`, which goes with its next push. Whether the connection must be
    /// told. A title or folder alert owes the pane's changes, or out of the client's view a push
    /// of its title and folder alone ([`Owed::names`])
    pub fn alert(&mut self, pane_id: PaneId, alert: Alert) -> bool {
        let hides = self.hides(pane_id);
        let owed = self.owe(pane_id);
        if mux::lbterm_output::reads_the_push(&alert) {
            if hides {
                owed.names = true;
            } else {
                owed.changes = true;
            }
        }
        note_alert(&mut owed.alerts, alert);
        self.ring_for(pane_id)
    }

    /// Whether `pane_id`'s lines are held when it prints: the client has said what it shows, it
    /// doesn't show this pane, and a push has carried the pane to it
    fn hides(&self, pane_id: PaneId) -> bool {
        self.shown
            .as_ref()
            .map_or(false, |shown| !shown.contains(&pane_id))
            && self.pushed.contains(&pane_id)
    }

    /// Whether [`Pending::take_next`] passes over `pane_id` now: a shown pane owed its changes
    /// whose last round push no frame has been said to read, and given no input since a frame
    /// last did (lbterm/MUX.md, "A shown pane's pace"). An owe of alerts alone isn't
    fn passes_over(&self, pane_id: PaneId) -> bool {
        self.unread.contains_key(&pane_id)
            && !self.input.contains_key(&pane_id)
            && self.owed.get(&pane_id).map_or(false, |owed| owed.changes)
    }

    /// The connection is told of what's owed `pane_id` only when it would push it: a pane passed
    /// over is pushed in the round after the client's word, which the connection reads itself,
    /// so it rings nothing, and a connection woken for it would find nothing to do (PITFALLS 228)
    fn ring_for(&mut self, pane_id: PaneId) -> bool {
        if self.passes_over(pane_id) {
            false
        } else {
            self.ring()
        }
    }

    /// A round pushed `pane_id`'s changes at `seqno`, at `now`: on a connection whose client says
    /// what its frames read, a pane it shows waits for the word before its next round push
    pub fn pushed_changes(&mut self, pane_id: PaneId, seqno: SequenceNo, now: Instant) {
        if self.paced && self.shown.as_ref().map_or(false, |shown| shown.contains(&pane_id)) {
            self.unread.insert(pane_id, (seqno, now));
        }
        if let Some(first @ None) = self.input.get_mut(&pane_id) {
            *first = Some(seqno);
        }
    }

    /// The client says a frame read `pane_id`'s push of `seqno` (codec's PanesDrawn), at `now`:
    /// it paces its shown panes from here, and the pane's last round push, when it's that one or
    /// older, has been read, so its next round push goes; and when that push was made since the
    /// pane's last input, the input's leave ends. How long the pane's changes, owed meanwhile,
    /// waited for this word (counted from the push it ends), when they did: the push log's
    /// `waited`
    pub fn drawn(&mut self, pane_id: PaneId, seqno: SequenceNo, now: Instant) -> Option<Duration> {
        self.paced = true;
        if matches!(self.input.get(&pane_id), Some(Some(first)) if *first <= seqno) {
            self.input.remove(&pane_id);
        }
        let at = match self.unread.get(&pane_id) {
            Some(&(pushed, at)) if pushed <= seqno => at,
            _ => return None,
        };
        self.unread.remove(&pane_id);
        if self.owed.get(&pane_id).map_or(false, |owed| owed.changes) {
            Some(now.saturating_duration_since(at))
        } else {
            None
        }
    }

    /// The client gave `pane_id` input at `now` (a key, a write, a paste, a mouse event): until a
    /// frame has read the first round push of it made since ([`Pending::pushed_changes`] notes
    /// its seqno, [`Pending::drawn`] ends the leave at a word for it or later), none of its round
    /// pushes waits for a frame's word, so the program's echo goes as it comes. The round after
    /// the input's PDU, which the connection reads itself, pushes the pane if it's owed
    pub fn input(&mut self, pane_id: PaneId, _now: Instant) {
        self.input.insert(pane_id, None);
    }

    /// Whether `pane_id`'s lines are held now
    pub fn holds(&self, pane_id: PaneId) -> bool {
        self.held.contains_key(&pane_id)
    }

    /// `pane_id`'s hold ends: its push is owed, everything it printed since its last in one go,
    /// at the front of the order when `front` (a pane the client has just shown), and the word
    /// of the hold not sent yet isn't sent. Whether it was held
    fn release(&mut self, pane_id: PaneId, front: bool) -> bool {
        if self.held.remove(&pane_id).is_none() {
            return false;
        }
        let owed = self.owe(pane_id);
        owed.changes = true;
        owed.held = false;
        if front {
            self.order.retain(|p| *p != pane_id);
            self.order.push_front(pane_id);
        }
        true
    }

    /// The holds of the panes quiet for [`config::lbterm::HELD_QUIET`] at `now` end, each pane
    /// owed its push: a session done and waiting has its screen in the window a quarter of a
    /// second after it stops. Whether any did
    pub fn release_quiet(&mut self, now: Instant) -> bool {
        let quiet: Vec<PaneId> = self
            .held
            .iter()
            .filter(|(_, last)| now.saturating_duration_since(**last) >= config::lbterm::HELD_QUIET)
            .map(|(pane_id, _)| *pane_id)
            .collect();
        let mut any = false;
        for pane_id in quiet {
            any |= self.release(pane_id, false);
        }
        any
    }

    /// When the next hold ends for being quiet, if the pane prints no more before: what the
    /// connection waits for beside news, its socket and its client (dispatch.rs)
    pub fn quiet_deadline(&self) -> Option<Instant> {
        self.held
            .values()
            .min()
            .map(|last| *last + config::lbterm::HELD_QUIET)
    }

    /// The client has `pane_id`'s lines as the pane has them now: a push carried them (a round's,
    /// a key's answer, a poll's), or the pane has printed nothing since its last. Its hold, if
    /// any, ends with nothing more owed for it, the word of the hold not sent yet isn't sent, and
    /// its holds can start from here
    pub fn current(&mut self, pane_id: PaneId) {
        self.pushed.insert(pane_id);
        self.held.remove(&pane_id);
        if let Some(owed) = self.owed.get_mut(&pane_id) {
            owed.held = false;
        }
    }

    /// `pane_id` has gone: nothing is owed or held for it
    pub fn removed(&mut self, pane_id: PaneId) {
        if self.owed.remove(&pane_id).is_some() {
            self.order.retain(|p| *p != pane_id);
        }
        self.held.remove(&pane_id);
        self.pushed.remove(&pane_id);
        self.unread.remove(&pane_id);
        self.input.remove(&pane_id);
    }

    fn owe(&mut self, pane_id: PaneId) -> &mut Owed {
        let order = &mut self.order;
        self.owed.entry(pane_id).or_insert_with(|| {
            order.push_back(pane_id);
            Owed::default()
        })
    }

    fn ring(&mut self) -> bool {
        !std::mem::replace(&mut self.rung, true)
    }

    /// The client's windows show `panes` (SetShownPanes), the whole set, in place of the one it
    /// said before. A pane held that's in it is owed its push at once, ahead of every other but
    /// the pane typed into, so a switch to a tab still printing has its screen as the server has
    /// it in one push, the next one made. A pane no longer shown waits for no frame's word: its
    /// lines are held as any pane's out of view, and when it's shown again its push goes at once.
    /// Whether the set of panes changed (each pane's `drawn` is read by nobody)
    pub fn set_shown(&mut self, panes: &[codec::ShownPane]) -> bool {
        let shown: HashSet<PaneId> = panes.iter().map(|p| p.pane_id).collect();
        let changed = self.shown.as_ref() != Some(&shown);
        self.unread.retain(|pane_id, _| shown.contains(pane_id));
        self.input.retain(|pane_id, _| shown.contains(pane_id));
        self.shown = Some(shown);
        // In reverse, each to the front, so they're pushed in the order the client said them
        for pane in panes.iter().rev() {
            self.release(pane.pane_id, true);
        }
        changed
    }

    /// Whether the client shows `pane_id`: in the set it last said, or any pane when it has said
    /// none (an older window, the cli)
    pub fn shows(&self, pane_id: PaneId) -> bool {
        self.shown.as_ref().map_or(true, |shown| shown.contains(&pane_id))
    }

    /// Whether the client has said what it shows, and so knows codec's PaneHeld
    pub fn says_shown(&self) -> bool {
        self.shown.is_some()
    }

    /// The client asked which tabs are parked (codec's GetParkedTabs)
    pub fn asked_parked(&mut self) {
        self.parked = true;
    }

    /// Whether the client knows codec's TabParked, having asked GetParkedTabs
    pub fn knows_parked(&self) -> bool {
        self.parked
    }

    /// The next pane owed a push and what it's owed: `first` ahead of the rest when it's owed
    /// one (the pane the client last typed into), else the one owed longest; a shown pane waiting
    /// for a frame's word that its last push was read is passed over, owed still, in its place
    /// ([`Pending::passes_over`])
    pub fn take_next(&mut self, first: Option<PaneId>) -> Option<(PaneId, Owed)> {
        let pane_id = match first {
            Some(id) if self.owed.contains_key(&id) && !self.passes_over(id) => {
                self.order.retain(|p| *p != id);
                id
            }
            _ => {
                let at = self.order.iter().position(|p| !self.passes_over(*p))?;
                self.order.remove(at)?
            }
        };
        let owed = self.owed.remove(&pane_id).unwrap_or_default();
        Some((pane_id, owed))
    }

    /// How many panes are owed a push
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// Whether anything is owed that a round would send now: a window's title, or a pane's push
    /// [`Pending::take_next`] wouldn't pass over (PITFALLS 228)
    pub fn owes(&self) -> bool {
        !self.titles.is_empty() || self.order.iter().any(|p| !self.passes_over(*p))
    }

    /// The connection is about to wait, its outbox holding `outbox`: what it must wait for. With
    /// no room in the outbox, the socket taking some is what lets it act, so news doesn't ring it:
    /// a flooded window's connection, its outbox full, would wake for each pane's next output and
    /// find nothing it could do. With room, news rings it from now on, and what it owes already it
    /// goes round for now, since nothing else would tell it ([`Wait::Now`])
    pub fn wait(&mut self, outbox: &Outbox) -> Wait {
        if !outbox.has_room() {
            return Wait::Writable;
        }
        self.rung = false;
        if self.owes() {
            Wait::Now
        } else if outbox.len() > 0 {
            Wait::Writable
        } else {
            Wait::News
        }
    }
}

/// Works out what a connection owes while its outbox has room: the windows' titles, then the
/// panes' pushes, `first` ahead of the rest (the pane the client last gave input to), each with
/// `push`. It stops as soon as `input_waits` says the client has sent something, so a key waits
/// for no more than the push being worked out when it came. The holds quiet at `now` end first,
/// each owing its pane's push, whether or not there's room for it yet
pub fn fill(
    pending: &Mutex<Pending>,
    outbox: &mut Outbox,
    first: Option<PaneId>,
    now: Instant,
    mut input_waits: impl FnMut() -> bool,
    mut push: impl FnMut(PaneId, Owed, &mut Outbox) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let titles = {
        let mut pending = pending.lock().unwrap();
        pending.release_quiet(now);
        if outbox.has_room() {
            pending.take_titles()
        } else {
            vec![]
        }
    };
    for (window_id, title) in titles {
        outbox.add_pdu(Pdu::WindowTitleChanged(codec::WindowTitleChanged {
            window_id,
            title,
        }))?;
    }
    while outbox.has_room() {
        if input_waits() {
            break;
        }
        let next = pending.lock().unwrap().take_next(first);
        match next {
            Some((pane_id, owed)) => push(pane_id, owed, outbox)?,
            None => break,
        }
    }
    Ok(())
}

/// The encoded PDUs a connection has for its client, written as the socket takes them
#[derive(Default)]
pub struct Outbox {
    buf: Vec<u8>,
    sent: usize,
}

impl Outbox {
    /// `pdu` encoded after what's here, as `Pdu::encode_async` would write it
    pub fn add(&mut self, decoded: &DecodedPdu) -> anyhow::Result<()> {
        decoded.pdu.encode(&mut self.buf, decoded.serial)
    }

    /// `pdu`, unilateral (serial 0), after what's here
    pub fn add_pdu(&mut self, pdu: Pdu) -> anyhow::Result<()> {
        pdu.encode(&mut self.buf, 0)
    }

    /// The PDUs not yet taken, decoded, as a client would read them, the outbox emptied
    #[cfg(test)]
    pub(crate) fn take_pdus(&mut self) -> Vec<Pdu> {
        let mut buf = self.buf.split_off(self.sent);
        self.buf.clear();
        self.sent = 0;
        let mut pdus = vec![];
        while let Some(decoded) = Pdu::stream_decode(&mut buf).unwrap() {
            pdus.push(decoded.pdu);
        }
        assert!(buf.is_empty(), "a part of a PDU left");
        pdus
    }

    /// The bytes not yet taken by the socket
    pub fn len(&self) -> usize {
        self.buf.len() - self.sent
    }

    /// Whether the connection should work out more of what it owes: what's here is under
    /// `OUTBOX_LOW`
    pub fn has_room(&self) -> bool {
        self.len() < OUTBOX_LOW
    }

    /// Writes what the socket takes now without waiting for it, and flushes once it has it
    /// all. A socket that takes nothing more leaves the rest for when it has room (the
    /// connection waits on it being writable).
    pub fn write_now<W: futures::AsyncWrite + Unpin>(&mut self, w: &mut W) -> std::io::Result<()> {
        use futures::{AsyncWriteExt, FutureExt};
        while self.sent < self.buf.len() {
            match w.write(&self.buf[self.sent..]).now_or_never() {
                None => break,
                Some(Ok(0)) => return Err(ErrorKind::WriteZero.into()),
                Some(Ok(n)) => self.sent += n,
                Some(Err(err)) if err.kind() == ErrorKind::WouldBlock => break,
                Some(Err(err)) if err.kind() == ErrorKind::Interrupted => {}
                Some(Err(err)) => return Err(err),
            }
        }
        if self.sent == self.buf.len() && self.sent > 0 {
            self.buf.clear();
            self.sent = 0;
            // A large answer (a page of scrollback) doesn't keep its room for ever
            if self.buf.capacity() > 1024 * 1024 {
                self.buf.shrink_to(64 * 1024);
            }
            match w.flush().now_or_never() {
                None | Some(Ok(())) => {}
                Some(Err(err)) if err.kind() == ErrorKind::WouldBlock => {}
                Some(Err(err)) => return Err(err),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    fn var(name: &str, value: &str) -> Alert {
        Alert::SetUserVar {
            name: name.to_string(),
            value: value.to_string(),
        }
    }

    /// A connection is told of parks only once its client has asked which tabs are parked: an
    /// older window never asks, and would end the connection on a TabParked (PITFALLS 188)
    #[test]
    fn parks_are_told_only_to_a_client_that_asked() {
        let pending = std::sync::Mutex::new(Pending::default());
        let park = || mux::MuxNotification::LbtermTabParked { tab_id: 2, parked: true };
        assert!(!pending.lock().unwrap().knows_parked());
        assert!(crate::dispatch::notification_pdu(park(), &pending).is_none());
        pending.lock().unwrap().asked_parked();
        assert!(matches!(
            crate::dispatch::notification_pdu(park(), &pending),
            Some(codec::Pdu::TabParked(codec::TabParked { tab_id: 2, parked: true }))
        ));
    }

    #[test]
    fn a_pane_is_owed_once_however_often_it_prints() {
        let mut p = Pending::default();
        assert!(p.output(3), "the first rings");
        for _ in 0..1000 {
            assert!(!p.output(3), "the rest don't, until the connection waits");
        }
        assert!(!p.output(5));
        assert_eq!(p.len(), 2);
        assert_eq!(p.take_next(None), Some((3, printed(vec![]))));
        assert!(!p.output(3), "nor while it works them out");
        assert_eq!(p.wait(&Outbox::default()), Wait::Now, "it owes them and has room");
        assert!(p.output(3), "once it has waited, the next rings again");
        assert!(!p.output(3));
        assert_eq!(p.take_next(None), Some((5, printed(vec![]))));
        assert_eq!(p.take_next(None), Some((3, printed(vec![]))), "owed once however often");
        assert_eq!(p.take_next(None), None);
        assert_eq!(p.len(), 0);
        assert_eq!(p.wait(&Outbox::default()), Wait::News);
    }

    #[test]
    fn a_windows_title_is_held_once_its_latest() {
        let mut p = Pending::default();
        assert!(p.window_title(0, "a".to_string()), "the first rings");
        for i in 0..1000 {
            assert!(!p.window_title(0, format!("block {i}")));
        }
        assert!(!p.window_title(2, "other".to_string()));
        assert_eq!(
            p.take_titles(),
            vec![(0, "block 999".to_string()), (2, "other".to_string())]
        );
        assert!(p.take_titles().is_empty());
        assert_eq!(p.take_next(None), None, "no pane is owed for a title");
        assert!(!p.window_title(0, "b".to_string()), "not until it has waited");
        assert!(p.owes(), "a title is owed");
        assert_eq!(p.wait(&Outbox::default()), Wait::Now);
        p.take_titles();
        assert_eq!(p.wait(&Outbox::default()), Wait::News);
        assert!(p.window_title(0, "c".to_string()), "once it has waited, the next rings");
    }

    fn shown(panes: &[(PaneId, Option<SequenceNo>)]) -> Vec<codec::ShownPane> {
        panes.iter().map(|&(pane_id, drawn)| codec::ShownPane { pane_id, drawn }).collect()
    }

    /// A client that never says what it shows (an older window, the cli) is taken to show every
    /// pane, as every client was before SetShownPanes; one that says shows what it said last,
    /// the whole set each time
    #[test]
    fn a_client_shows_every_pane_until_it_says_what_it_shows() {
        let mut pending = Pending::default();
        assert!(!pending.says_shown());
        assert!(pending.shows(1) && pending.shows(99));

        assert!(pending.set_shown(&shown(&[(3, None), (4, None)])), "the first set said");
        assert!(pending.says_shown());
        assert!(pending.shows(3) && pending.shows(4));
        assert!(!pending.shows(1), "a pane of a tab out of view");

        // A switch: the set said again replaces the last
        assert!(pending.set_shown(&shown(&[(1, None)])));
        assert!(pending.shows(1) && !pending.shows(3) && !pending.shows(4));
        // The same panes again, or with a frame's seqno, aren't a change of what's shown
        assert!(!pending.set_shown(&shown(&[(1, None)])));
        assert!(!pending.set_shown(&shown(&[(1, Some(40))])));
        // No pane of this server shown (the windows' active tabs are all another domain's, say):
        // still a set, which shows none
        assert!(pending.set_shown(&[]));
        assert!(pending.says_shown() && !pending.shows(1));

        // Saying it holds nothing of a pane no push has carried yet: its first push goes
        assert!(pending.output(3));
        assert_eq!(pending.take_next(None).map(|(p, o)| (p, o.changes)), Some((3, true)));
    }

    /// What's owed, pane by pane, taken in the order a round takes it: whether its changes are
    /// worked out, its title and folder alone, the word of its hold, and its alerts
    fn takes(p: &mut Pending, first: Option<PaneId>) -> Vec<(PaneId, bool, bool, bool, usize)> {
        std::iter::from_fn(|| p.take_next(first))
            .map(|(id, o)| (id, o.changes, o.names, o.held, o.alerts.len()))
            .collect()
    }

    /// A client showing pane 1, which has been pushed panes 1 to 4 (each owe of them taken)
    fn showing_one() -> Pending {
        let mut p = Pending::default();
        p.set_shown(&shown(&[(1, None)]));
        for pane_id in 1..=4 {
            p.current(pane_id);
        }
        p
    }

    /// A pane out of view prints: no push of it is worked out; the client is told once that its
    /// lines are held, and the hold's later output doesn't wake the connection
    #[test]
    fn a_pane_out_of_view_has_its_lines_held_and_the_client_told_once() {
        let mut p = showing_one();
        assert!(p.output(2), "a hold's first output rings: the client is to be told");
        for _ in 0..1000 {
            assert!(!p.output(2));
        }
        assert!(p.holds(2));
        assert_eq!(takes(&mut p, None), vec![(2, false, false, true, 0)], "the word, no push");
        assert_eq!(p.wait(&Outbox::default()), Wait::News, "nothing more owed");
        for _ in 0..1000 {
            assert!(!p.output(2), "a hold told of doesn't ring the connection again");
        }
        assert!(!p.owes());
        assert!(p.holds(2), "still held");
        // The pane shown prints as before, and so does one no push has carried yet
        assert!(p.output(1));
        p.output(7);
        assert_eq!(
            takes(&mut p, None),
            vec![(1, true, false, false, 0), (7, true, false, false, 0)]
        );
        assert!(!p.holds(1) && !p.holds(7));
    }

    /// A held pane's alerts go at once without its lines: a bell or a user var as alerts alone,
    /// a title or folder change as a push of them alone ahead of its alerts
    #[test]
    fn a_held_panes_alerts_go_at_once_and_its_title_alone() {
        let mut p = showing_one();
        p.output(2);
        p.take_next(None);
        p.alert(2, Alert::Bell);
        p.alert(2, var("claude_status", "working"));
        assert_eq!(takes(&mut p, None), vec![(2, false, false, false, 2)]);
        p.alert(2, Alert::WindowTitleChanged("t".to_string()));
        p.alert(3, Alert::CurrentWorkingDirectoryChanged);
        assert_eq!(
            takes(&mut p, None),
            vec![(2, false, true, false, 1), (3, false, true, false, 1)],
            "a title or folder alone, held or not, out of view"
        );
        assert!(p.holds(2), "still held: none of that brought its lines");
        // Shown, a title owes the pane's changes as before
        p.alert(1, Alert::WindowTitleChanged("t".to_string()));
        assert_eq!(takes(&mut p, None), vec![(1, true, false, false, 1)]);
    }

    /// Shown, a held pane is owed one push, carrying everything since its last, first of all
    /// but the pane typed into, in the order the client said them
    #[test]
    fn a_held_pane_shown_is_pushed_once_ahead_of_the_rest() {
        let mut p = showing_one();
        p.output(1);
        for _ in 0..100 {
            p.output(2);
            p.output(4);
        }
        p.alert(4, Alert::Bell);
        p.output(3);
        p.take_next(Some(1)); // pane 1's push
        // 2's and 4's word of the hold, 3's too, not taken yet; 4 with its bell
        assert!(p.set_shown(&shown(&[(4, None), (2, None)])), "a switch to a split tab");
        assert!(!p.holds(2) && !p.holds(4));
        assert_eq!(
            takes(&mut p, Some(1)),
            vec![(4, true, false, false, 1), (2, true, false, false, 0), (3, false, false, true, 0)],
            "4 and 2 pushed whole with no word of a hold that's over; 3 still held"
        );
        assert!(p.holds(3));
        // The panes hidden by the switch hold from their next output on
        p.output(1);
        assert_eq!(takes(&mut p, None), vec![(1, false, false, true, 0)]);
        // A switch that shows nothing held owes nothing
        assert!(p.set_shown(&shown(&[(5, None)])));
        assert!(!p.owes());
    }

    /// A held pane that prints nothing for HELD_QUIET is pushed, everything since in one push
    #[test]
    fn a_held_pane_quiet_for_a_while_is_pushed() {
        let quiet = config::lbterm::HELD_QUIET;
        let t0 = Instant::now();
        let ms = |n| std::time::Duration::from_millis(n);
        let mut p = showing_one();
        assert_eq!(p.quiet_deadline(), None);
        p.output_at(2, t0);
        p.output_at(3, t0 + ms(50));
        p.output_at(2, t0 + ms(100));
        p.take_next(None);
        p.take_next(None);
        assert_eq!(p.quiet_deadline(), Some(t0 + ms(50) + quiet), "the soonest: pane 3's");
        assert!(!p.release_quiet(t0 + ms(50) + quiet - ms(1)));
        assert!(p.release_quiet(t0 + ms(50) + quiet));
        assert_eq!(takes(&mut p, None), vec![(3, true, false, false, 0)]);
        assert_eq!(p.quiet_deadline(), Some(t0 + ms(100) + quiet), "2 printed again since");
        assert!(p.release_quiet(t0 + ms(100) + quiet));
        assert_eq!(takes(&mut p, None), vec![(2, true, false, false, 0)]);
        assert_eq!(p.quiet_deadline(), None);
    }

    /// A round ends the holds gone quiet before it works out what's owed, room or not
    #[test]
    fn a_round_ends_the_holds_gone_quiet() {
        let t0 = Instant::now();
        let mut p = showing_one();
        p.output_at(2, t0);
        p.take_next(None);
        let pending = Mutex::new(p);
        let mut full = Outbox::default();
        full.buf = vec![0; OUTBOX_LOW];
        let later = t0 + config::lbterm::HELD_QUIET;
        fill(&pending, &mut full, None, later, || false, |_, _, _| panic!("no room")).unwrap();
        assert!(!pending.lock().unwrap().holds(2), "ended though the outbox is full");
        let mut out = Outbox::default();
        let mut pushed = vec![];
        fill(&pending, &mut out, None, later, || false, |pane_id, owed, _| {
            pushed.push((pane_id, owed.changes));
            Ok(())
        })
        .unwrap();
        assert_eq!(pushed, vec![(2, true)]);
    }

    /// A push that carries a held pane's lines (a poll's, a key's) ends its hold, the word of it
    /// not sent yet goes unsent, and its next output starts another hold and its word
    #[test]
    fn a_push_that_carries_a_held_panes_lines_ends_its_hold() {
        let mut p = showing_one();
        p.output(2);
        p.alert(2, Alert::Bell);
        p.current(2);
        assert!(!p.holds(2));
        assert_eq!(takes(&mut p, None), vec![(2, false, false, false, 1)], "the bell alone");
        assert_eq!(p.wait(&Outbox::default()), Wait::News);
        assert!(p.output(2), "a new hold's first output rings");
        assert_eq!(takes(&mut p, None), vec![(2, false, false, true, 0)], "a hold again");
        p.current(2);
        assert_eq!(p.quiet_deadline(), None);
    }

    /// A pane whose push is owed when it goes out of view is still pushed whole, its output
    /// meanwhile with it, and holds once that push has gone
    #[test]
    fn a_push_owed_as_a_pane_goes_out_of_view_still_goes() {
        let mut p = showing_one();
        p.output(1);
        p.set_shown(&shown(&[(2, None)]));
        p.output(1);
        assert!(!p.holds(1));
        assert_eq!(takes(&mut p, None), vec![(1, true, false, false, 0)]);
        p.current(1);
        p.output(1);
        assert!(p.holds(1));
    }

    /// Each connection holds what its own client doesn't show: a pane shown in one window and
    /// not the other is pushed to the first alone
    #[test]
    fn two_connections_each_hold_what_their_own_client_doesnt_show() {
        let mut a = showing_one();
        let mut b = showing_one();
        b.set_shown(&shown(&[(2, None)]));
        for p in [&mut a, &mut b] {
            p.output(1);
            p.output(2);
        }
        assert_eq!(takes(&mut a, None), vec![(1, true, false, false, 0), (2, false, false, true, 0)]);
        assert_eq!(takes(&mut b, None), vec![(1, false, false, true, 0), (2, true, false, false, 0)]);
        assert!(a.holds(2) && !a.holds(1));
        assert!(b.holds(1) && !b.holds(2));
    }

    /// A client that never says what it shows (an older window, the cli) is pushed every pane as
    /// it prints, with nothing held and no word of a hold, as before SetShownPanes
    #[test]
    fn a_client_that_never_says_what_it_shows_is_pushed_everything() {
        let mut p = Pending::default();
        for pane_id in 1..=4 {
            p.current(pane_id);
        }
        for _ in 0..10 {
            for pane_id in 1..=4 {
                p.output(pane_id);
            }
        }
        p.alert(3, Alert::WindowTitleChanged("t".to_string()));
        assert_eq!(
            takes(&mut p, None),
            vec![
                (1, true, false, false, 0),
                (2, true, false, false, 0),
                (3, true, false, false, 1),
                (4, true, false, false, 0)
            ]
        );
        assert!((1..=4).all(|pane_id| !p.holds(pane_id)));
        assert_eq!(p.quiet_deadline(), None);
    }

    /// A pane gone is neither owed nor held, nor waited on for being quiet
    #[test]
    fn a_pane_gone_is_neither_owed_nor_held() {
        let mut p = showing_one();
        p.output(2);
        p.output(3);
        p.alert(3, Alert::Bell);
        p.removed(3);
        p.removed(2);
        assert!(!p.holds(2) && !p.owes());
        assert_eq!(p.quiet_deadline(), None);
    }

    #[test]
    fn the_pane_typed_into_goes_first() {
        let mut p = Pending::default();
        for id in [1, 2, 3, 4] {
            p.output(id);
        }
        assert_eq!(p.take_next(Some(3)).map(|t| t.0), Some(3));
        assert_eq!(p.take_next(Some(3)).map(|t| t.0), Some(1), "not owed: the oldest");
        assert_eq!(p.take_next(Some(9)).map(|t| t.0), Some(2));
        assert_eq!(p.take_next(None).map(|t| t.0), Some(4));
        assert_eq!(p.take_next(Some(4)), None);
    }

    #[test]
    fn alerts_go_with_their_panes_push_and_owe_it() {
        let mut p = Pending::default();
        assert!(p.alert(7, Alert::Bell));
        assert!(!p.output(7));
        assert!(!p.alert(8, var("a", "1")));
        assert_eq!(p.take_next(None), Some((7, printed(vec![Alert::Bell]))));
        assert_eq!(
            p.take_next(None),
            Some((
                8,
                Owed {
                    alerts: vec![var("a", "1")],
                    ..Owed::default()
                }
            ))
        );
    }

    fn printed(alerts: Vec<Alert>) -> Owed {
        Owed {
            alerts,
            changes: true,
            ..Owed::default()
        }
    }

    /// An owe of alerts alone has the pane's changes worked out only for an alert whose receiver
    /// reads what the push carries (a title, the cwd); output always has them worked out, whatever
    /// came before or after it while it was owed
    #[test]
    fn alerts_alone_are_sent_without_the_panes_changes_unless_they_read_them() {
        let mut p = Pending::default();
        p.alert(1, var("a", "1"));
        p.alert(1, Alert::Bell);
        p.alert(2, var("b", "2"));
        p.alert(2, Alert::WindowTitleChanged("t".to_string()));
        p.alert(3, Alert::CurrentWorkingDirectoryChanged);
        p.alert(4, Alert::Progress(wezterm_term::Progress::Percentage(1)));
        p.output(4);
        p.output(5);
        p.alert(5, var("c", "3"));
        let changes: Vec<(PaneId, bool)> = std::iter::from_fn(|| p.take_next(None))
            .map(|(id, owed)| (id, owed.changes))
            .collect();
        assert_eq!(
            changes,
            vec![(1, false), (2, true), (3, true), (4, true), (5, true)]
        );
        // Taken, a pane owed again starts afresh
        p.output(1);
        p.alert(2, Alert::Bell);
        assert_eq!(p.take_next(None).map(|t| t.1.changes), Some(true));
        assert_eq!(p.take_next(None).map(|t| t.1.changes), Some(false));
    }

    /// A socket that takes `room` bytes and then would block until it's given more
    struct Socket {
        took: Vec<u8>,
        room: usize,
        flushes: usize,
    }

    impl futures::AsyncWrite for Socket {
        fn poll_write(mut self: Pin<&mut Self>, _cx: &mut Context, buf: &[u8]) -> Poll<std::io::Result<usize>> {
            if self.room == 0 {
                return Poll::Pending;
            }
            let n = buf.len().min(self.room).min(1000);
            self.room -= n;
            self.took.extend_from_slice(&buf[..n]);
            Poll::Ready(Ok(n))
        }
        fn poll_flush(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<std::io::Result<()>> {
            self.flushes += 1;
            Poll::Ready(Ok(()))
        }
        fn poll_close(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[test]
    fn the_outbox_writes_what_the_socket_takes_and_keeps_the_rest_in_order() {
        let mut out = Outbox::default();
        let mut want = vec![];
        for i in 0..20u64 {
            let pdu = DecodedPdu {
                serial: i,
                pdu: Pdu::Pong(codec::Pong {}),
            };
            out.add(&pdu).unwrap();
            pdu.pdu.encode(&mut want, i).unwrap();
        }
        assert_eq!(out.len(), want.len());
        let mut sock = Socket {
            took: vec![],
            room: 7,
            flushes: 0,
        };
        out.write_now(&mut sock).unwrap();
        assert_eq!(sock.took.len(), 7);
        assert_eq!(out.len(), want.len() - 7);
        assert_eq!(sock.flushes, 0, "nothing flushed until it's all gone");
        out.write_now(&mut sock).unwrap();
        assert_eq!(sock.took.len(), 7, "a full socket takes nothing");
        sock.room = usize::MAX;
        out.write_now(&mut sock).unwrap();
        assert_eq!(sock.took, want, "the bytes as encode writes them, in order");
        assert_eq!(out.len(), 0);
        assert_eq!(sock.flushes, 1);
        assert!(out.has_room());
    }

    /// What a writer gets into a socket with nobody reading it, before it would block
    fn holds(stream: &mut wezterm_uds::UnixStream) -> usize {
        use std::io::Write;
        stream.set_nonblocking(true).unwrap();
        let buf = [0u8; 1024];
        let mut total = 0;
        loop {
            match stream.write(&buf) {
                Ok(n) => total += n,
                Err(err) if err.kind() == ErrorKind::WouldBlock => return total,
                Err(err) => panic!("{:#}", err),
            }
            assert!(total < 64 * 1024 * 1024, "a socket that never fills");
        }
    }

    #[test]
    fn a_local_socket_holds_its_send_buffer_and_no_more() {
        let dir = std::env::temp_dir().join(format!("lbflow{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s");
        let listener = wezterm_uds::UnixListener::bind(&path).unwrap();
        let _client = wezterm_uds::UnixStream::connect(&path).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let _client2 = wezterm_uds::UnixStream::connect(&path).unwrap();
        let (mut server2, _) = listener.accept().unwrap();
        set_send_buffer(&server).unwrap();
        let held = holds(&mut server);
        let default = holds(&mut server2);
        drop(listener);
        std::fs::remove_dir_all(&dir).ok();
        assert!(held >= SEND_BUFFER / 2 && held <= SEND_BUFFER + 1024, "{} bytes in with {} asked", held, SEND_BUFFER);
        assert!(default > held, "the default ({}) holds more", default);
    }

    /// A connection's rounds as dispatch.rs goes through them, with no news after the panes owed
    /// at the start and each push `push_bytes` long, against a socket that takes `room` bytes and
    /// then `room` more each time it's waited on: the panes pushed, in order, and the bytes the
    /// socket took, once the connection would wait for news
    fn rounds(pending: &Mutex<Pending>, room: usize, push_bytes: usize) -> (Vec<PaneId>, usize) {
        let mut out = Outbox::default();
        let mut sock = Socket {
            took: vec![],
            room,
            flushes: 0,
        };
        let mut pushed = vec![];
        for _ in 0..100_000 {
            fill(pending, &mut out, None, Instant::now(), || false, |pane_id, _, o| {
                pushed.push(pane_id);
                o.buf.extend(std::iter::repeat(pane_id as u8).take(push_bytes));
                Ok(())
            })
            .unwrap();
            out.write_now(&mut sock).unwrap();
            match pending.lock().unwrap().wait(&out) {
                Wait::Now => {}
                Wait::Writable => sock.room += room,
                Wait::News => return (pushed, sock.took.len()),
            }
        }
        panic!("the connection never waited for news");
    }

    fn owing(panes: std::ops::Range<PaneId>) -> Mutex<Pending> {
        let mut p = Pending::default();
        for pane_id in panes {
            p.output(pane_id);
        }
        Mutex::new(p)
    }

    #[test]
    fn every_push_owed_goes_out_whatever_the_socket_takes_at_once() {
        // A round stops at OUTBOX_LOW with panes still owed; a socket with room takes the lot at
        // once, so nothing says the socket can take more, and no news comes: the connection must
        // go round again rather than wait (the lost wake of 2026-10-31)
        for (room, push_bytes) in [
            (1 << 30, 1500),
            (1 << 30, 300),
            (1 << 30, OUTBOX_LOW * 3),
            (SEND_BUFFER, 1500),
            (100, 1500),
            (7, 300),
        ] {
            let pending = owing(0..6);
            let (pushed, took) = rounds(&pending, room, push_bytes);
            assert_eq!(pushed, (0..6).collect::<Vec<_>>(), "room {room}, pushes of {push_bytes}");
            assert_eq!(took, 6 * push_bytes, "room {room}, pushes of {push_bytes}");
            assert!(!pending.lock().unwrap().owes());
        }
    }

    #[test]
    fn titles_that_fill_the_outbox_leave_no_push_unsent() {
        let mut p = Pending::default();
        for window_id in 0..200 {
            p.window_title(window_id, format!("a title long enough to fill it {window_id}"));
        }
        for pane_id in 0..3 {
            p.output(pane_id);
        }
        let pending = Mutex::new(p);
        let (pushed, _) = rounds(&pending, 1 << 30, 1500);
        assert_eq!(pushed, vec![0, 1, 2]);
        // And news after it waits rings it
        assert!(pending.lock().unwrap().output(9));
    }

    #[test]
    fn a_round_stops_for_the_clients_input() {
        let pending = owing(0..4);
        let mut out = Outbox::default();
        let mut pushed = vec![];
        let mut asked = 0;
        fill(
            &pending,
            &mut out,
            Some(2),
            Instant::now(),
            || {
                asked += 1;
                asked > 1
            },
            |pane_id, _, o| {
                pushed.push(pane_id);
                o.buf.extend_from_slice(&[0; 10]);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(pushed, vec![2], "the pane typed into, then the client's input");
        assert_eq!(
            pending.lock().unwrap().wait(&out),
            Wait::Now,
            "the rest are owed, for after the input (dispatch.rs reads the client first)"
        );
    }

    #[test]
    fn the_outbox_has_room_below_its_mark() {
        let mut out = Outbox::default();
        assert!(out.has_room());
        out.buf = vec![0; OUTBOX_LOW];
        assert!(!out.has_room());
        out.sent = 1;
        assert!(out.has_room());
    }

    #[test]
    fn news_doesnt_ring_a_connection_with_no_room() {
        let mut p = Pending::default();
        let mut full = Outbox::default();
        full.buf = vec![0; OUTBOX_LOW];
        assert!(p.output(1), "the first rings");
        assert_eq!(p.wait(&full), Wait::Writable, "it waits for the socket");
        for _ in 0..100 {
            assert!(!p.output(2), "news it couldn't act on doesn't wake it");
        }
        // The socket took some: it works the pushes out and waits with room again
        full.sent = full.buf.len();
        assert_eq!(p.wait(&full), Wait::Now, "it owes them and has room");
        assert!(p.output(3), "and news rings it from then on");
    }

    /// A client showing panes 1 and 2, all pushed, that says what its frames read (it has said
    /// one word, for nothing pushed yet): 1's and 2's round pushes are paced from here
    fn pacing() -> Pending {
        let mut p = Pending::default();
        p.set_shown(&shown(&[(1, None), (2, None)]));
        for pane_id in 1..=3 {
            p.current(pane_id);
        }
        assert_eq!(p.drawn(1, 0, Instant::now()), None, "nothing was waiting");
        p
    }

    /// What a round takes of a pane, as (pane, changes, alerts)
    fn took(p: &mut Pending) -> Vec<(PaneId, bool, usize)> {
        std::iter::from_fn(|| p.take_next(None))
            .map(|(id, o)| (id, o.changes, o.alerts.len()))
            .collect()
    }

    /// A shown pane pushed waits for the client's word that a frame read the push before its
    /// next: its output is owed, in one owe, and rings nothing; nothing counts as owed, so the
    /// connection waits for news (PITFALLS 228); a word for an older push ends nothing, and the
    /// word for that push or a later one lets the next push go, carrying everything since
    #[test]
    fn a_shown_panes_next_push_waits_for_the_word_that_a_frame_read_its_last() {
        let mut p = pacing();
        let t0 = Instant::now();
        assert!(p.output(1));
        assert_eq!(took(&mut p), vec![(1, true, 0)]);
        p.pushed_changes(1, 10, t0);
        assert_eq!(p.wait(&Outbox::default()), Wait::News);

        for _ in 0..1000 {
            assert!(!p.output(1), "output while the push is unread rings nothing");
        }
        assert_eq!(p.len(), 1, "owed once");
        assert!(!p.owes(), "but nothing a round would send");
        assert_eq!(p.wait(&Outbox::default()), Wait::News, "so no round for it");
        assert_eq!(p.take_next(None), None);
        assert_eq!(p.take_next(Some(1)), None, "not as the pane typed into either");

        // Another pane shown, and one out of view's word, still go meanwhile
        assert!(p.output(2));
        assert_eq!(took(&mut p), vec![(2, true, 0)]);
        p.output(3);
        assert_eq!(p.take_next(None).map(|(id, o)| (id, o.held)), Some((3, true)));

        // A frame that read the push before it: the unread one is still on its way
        assert_eq!(p.drawn(1, 9, t0), None);
        assert_eq!(p.take_next(None), None);
        // The frame that read it: the owe goes, once, with everything since
        let waited = p.drawn(1, 10, t0 + Duration::from_millis(12));
        assert_eq!(waited, Some(Duration::from_millis(12)), "how long the changes waited");
        assert!(p.owes());
        assert_eq!(took(&mut p), vec![(1, true, 0)]);
        // A word for a push read already, or a pane waiting for none, is nothing
        assert_eq!(p.drawn(1, 11, t0), None);
        assert_eq!(p.drawn(2, 4, t0), None);
    }

    /// A word that comes before the pane prints again leaves nothing to wait for: its next output
    /// rings, and its push goes
    #[test]
    fn a_pane_whose_push_was_read_before_it_printed_again_is_pushed_as_it_prints() {
        let mut p = pacing();
        let t0 = Instant::now();
        p.output(1);
        took(&mut p);
        p.pushed_changes(1, 10, t0);
        assert_eq!(p.drawn(1, 10, t0), None, "nothing owed waited for it");
        p.wait(&Outbox::default());
        assert!(p.output(1), "it rings");
        assert_eq!(took(&mut p), vec![(1, true, 0)]);
    }

    /// A client that never says what its frames read is pushed every change as it comes, as
    /// before the pace: an older window, the cli
    #[test]
    fn a_client_that_never_says_what_its_frames_read_is_pushed_every_change() {
        let mut p = Pending::default();
        p.set_shown(&shown(&[(1, None)]));
        p.current(1);
        for seqno in 1..20 {
            assert!(p.output(1));
            assert_eq!(took(&mut p), vec![(1, true, 0)]);
            p.pushed_changes(1, seqno, Instant::now());
            assert_eq!(p.wait(&Outbox::default()), Wait::News);
        }
        // Nor one that never said what it shows (the cli), whatever its frames say
        let mut cli = Pending::default();
        cli.drawn(1, 0, Instant::now());
        cli.output(1);
        took(&mut cli);
        cli.pushed_changes(1, 3, Instant::now());
        cli.output(1);
        assert_eq!(took(&mut cli), vec![(1, true, 0)], "only the panes said shown are paced");
    }

    /// An owe of alerts alone goes while the pane's push is unread: it reads nothing and carries
    /// no lines, so a shown pane's bell or user var is heard as soon as before. A title owes the
    /// pane's changes, and waits with them
    #[test]
    fn a_shown_panes_alerts_alone_go_while_it_waits_for_a_frame() {
        let mut p = pacing();
        let t0 = Instant::now();
        p.output(1);
        took(&mut p);
        p.pushed_changes(1, 10, t0);
        p.wait(&Outbox::default());
        assert!(p.alert(1, var("claude_status", "working")), "it rings");
        assert_eq!(took(&mut p), vec![(1, false, 1)], "the alert alone, no changes");

        p.wait(&Outbox::default());
        assert!(!p.output(1));
        assert!(!p.alert(1, var("claude_status", "idle")), "with changes owed, it waits too");
        assert!(!p.alert(1, Alert::WindowTitleChanged("t".to_string())));
        assert_eq!(p.take_next(None), None);
        p.drawn(1, 10, t0);
        assert_eq!(took(&mut p), vec![(1, true, 2)], "changes and both alerts in one");
    }

    /// Input to a pane lets its round pushes go as it prints until a frame has read one made
    /// after the input, so a key's echo waits for no frame, however fast the keys come; a word
    /// for a push made before the input doesn't end that
    #[test]
    fn input_lets_a_panes_pushes_go_until_a_frame_reads_one_made_since() {
        let mut p = pacing();
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        p.output(1);
        took(&mut p);
        p.pushed_changes(1, 10, ms(0));
        p.wait(&Outbox::default());
        assert!(!p.output(1), "unread, owed: passed over");

        // A key: what's owed goes in the round after the key's PDU
        p.input(1, ms(2));
        assert!(p.owes());
        // The frame that read the push made before the key: the key's leave goes on
        assert_eq!(p.drawn(1, 10, ms(3)), Some(Duration::from_millis(3)));
        assert_eq!(took(&mut p), vec![(1, true, 0)]);
        p.pushed_changes(1, 11, ms(4));
        p.wait(&Outbox::default());
        // The echo, though no frame has read the push before it
        assert!(p.output(1), "the echo rings");
        assert_eq!(took(&mut p), vec![(1, true, 0)]);
        p.pushed_changes(1, 12, ms(5));
        p.wait(&Outbox::default());
        assert!(p.output(1), "and the rest of it");
        assert_eq!(took(&mut p), vec![(1, true, 0)]);
        p.pushed_changes(1, 13, ms(6));

        // The frame that read a push made after the key: paced again
        assert_eq!(p.drawn(1, 13, ms(9)), None);
        p.wait(&Outbox::default());
        assert!(p.output(1));
        took(&mut p);
        p.pushed_changes(1, 14, ms(10));
        p.wait(&Outbox::default());
        assert!(!p.output(1), "waits for the frame again");
        assert_eq!(p.take_next(None), None);
    }

    /// Under a flood the pushes after a key go on faster than frames read them, so a word for the
    /// latest never comes: the word for the first push made since the key ends the leave
    /// (perf/mux-pace.ps1, 2026-11-09, where a key early in a flood left the pane unpaced)
    #[test]
    fn a_word_for_the_first_push_since_the_input_paces_again_under_a_flood() {
        let mut p = pacing();
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        p.output(1);
        took(&mut p);
        p.pushed_changes(1, 10, ms(0));
        p.input(1, ms(1));
        // Pushes go as it prints, each newer than the frames have read
        for (i, seqno) in [11usize, 12, 13].iter().copied().enumerate() {
            p.wait(&Outbox::default());
            assert!(p.output(1), "push {i} after the key goes");
            took(&mut p);
            p.pushed_changes(1, seqno, ms(2 + i as u64));
        }
        // The frame read 11, the first made since the key: paced again, 13 still unread
        assert_eq!(p.drawn(1, 11, ms(8)), None);
        p.wait(&Outbox::default());
        assert!(!p.output(1), "13 unread: waits for the frame");
        assert_eq!(p.take_next(None), None);
        assert!(p.drawn(1, 13, ms(9)).is_some(), "the word for 13 ends its wait");
        assert_eq!(took(&mut p), vec![(1, true, 0)]);
    }

    /// A pane that leaves the set said (a switch away) waits for no frame's word: its lines are
    /// held as any pane's out of view, and shown again it's pushed at once, at the front
    #[test]
    fn a_pane_shown_again_waits_for_no_word_from_before() {
        let mut p = pacing();
        let t0 = Instant::now();
        p.output(1);
        took(&mut p);
        p.pushed_changes(1, 10, t0);
        p.input(1, t0);
        p.wait(&Outbox::default());
        p.set_shown(&shown(&[(2, None)]));
        assert!(p.output(1), "held now: the word of its hold rings");
        assert_eq!(p.take_next(None).map(|(id, o)| (id, o.held)), Some((1, true)));
        p.output(2);
        p.set_shown(&shown(&[(1, None), (2, None)]));
        assert_eq!(took(&mut p), vec![(1, true, 0), (2, true, 0)], "1 at once, at the front");
        // It paces again from its next push, the old input gone with the switch
        p.pushed_changes(1, 20, t0);
        p.wait(&Outbox::default());
        assert!(!p.output(1));
        assert_eq!(p.take_next(None), None);
    }

    /// A pane gone waits for nothing and leaves nothing behind
    #[test]
    fn a_pane_gone_leaves_no_wait() {
        let mut p = pacing();
        p.output(1);
        took(&mut p);
        p.pushed_changes(1, 10, Instant::now());
        p.output(1);
        p.removed(1);
        assert!(!p.owes());
        assert_eq!(p.len(), 0);
        assert_eq!(p.drawn(1, 10, Instant::now()), None);
    }

    /// A round with only a pane it passes over sends nothing and waits for news: it never goes
    /// round again for it (PITFALLS 228), and once the word comes the next round pushes it
    #[test]
    fn a_round_doesnt_go_round_for_a_pane_waiting_for_a_frame() {
        let mut p = pacing();
        let t0 = Instant::now();
        p.output(1);
        took(&mut p);
        p.pushed_changes(1, 10, t0);
        p.output(1);
        let pending = Mutex::new(p);
        let mut out = Outbox::default();
        let mut pushed = vec![];
        let round = |pushed: &mut Vec<PaneId>, out: &mut Outbox| {
            fill(&pending, out, Some(1), Instant::now(), || false, |pane_id, _, _| {
                pushed.push(pane_id);
                Ok(())
            })
            .unwrap();
            pending.lock().unwrap().wait(out)
        };
        assert_eq!(round(&mut pushed, &mut out), Wait::News);
        assert!(pushed.is_empty());
        pending.lock().unwrap().drawn(1, 10, t0);
        assert_eq!(round(&mut pushed, &mut out), Wait::News);
        assert_eq!(pushed, vec![1]);
    }
}
