//! LBTerm: the Resize requests, each pane resized off the main thread to the newest size asked
//! for it (lbterm/MUX.md, "A window's resize"; the audit's server10).
//!
//! A window sends a Resize for each pane of each tab it resizes, one for every size it takes,
//! and upstream's server applied every one on its main thread in the order they came: the pane's
//! pseudoconsole resized, then its terminal fitting its whole scrollback to the new width
//! (`Screen::rewrap_lines`, about 70 ms a pane with 50,000 lines), then the tab's split sizes
//! rebuilt and a TabResized sent to every client. Requests that came while it was busy waited
//! behind each other, each still applied, and every other pane's keys and output waited behind
//! them all: a drag of the window's frame through 40 sizes with 12 such tabs held every tab's
//! echo for 35 s, and a window maximized with them for about a second (perf/mux-resize.ps1,
//! 2026-11-05).
//!
//! Now each request notes the newest size asked for its pane and its answer ([`Resizes::ask`],
//! the server's one [`resizes`], whichever connection asked, so two windows resizing a pane
//! still resize it one size at a time), and a pane is resized on a thread of its own
//! (`lbterm-resize`), one resize at a time a pane, the panes side by side: the main thread only
//! starts it and, when it's done, answers. Requests that come while a pane is being resized wait
//! for it, and the newest of them is applied after it, once; the rest are answered with it. A
//! size the pane has already ([`is_size`]) isn't applied again: its pseudoconsole would be
//! resized for nothing and its program would redraw. Each request is answered after the size it
//! asked for (or a newer one) was applied, as upstream answered, though no longer in the order
//! they came: a client matches an answer to its request by its serial. A resize that panics on
//! its thread is answered as an error naming the pane ([`guarded`]), so the pane's requests
//! behind it still run (PITFALLS 161).
//!
//! A tab's split sizes are rebuilt from its panes' (`rebuild_splits_sizes_from_contained_panes`,
//! which reads every pane of the tab under its terminal's lock, and says TabResized when the
//! layout moved) once none of its panes is being resized: rebuilt as each pane's resize ended,
//! the main thread waited there for a pane of the same tab still rewrapping on its thread, every
//! other pane's keys and pushes with it, and a client heard of a layout half-way through.
//!
//! A layout that moved only by one connection's own Resizes isn't news to that connection, which
//! laid it out itself: its TabResized goes to every other connection ([`is_own`], asked by each
//! connection's subscriber in dispatch.rs, keyed by [`Conn`]). Upstream told the client that asked
//! too, and every client answers a TabResized with a resync (ListPanes, every tab synced to the
//! server's sizes), which a window of any build before perf/mux-client follows by sending each
//! size it's listed that isn't its own back as a Resize (`ClientPane::resize`). During a drag its
//! next size had often gone before the listing came, so it sent the older size after it: with
//! ListPanes held (below) the installed window on this server ended a drag 4 to 20 columns short
//! of its window, the sent-back size the newest the book was asked, and with it unheld the drag
//! never ended (PITFALLS 159). Told nothing of its own, it ends at its window's size.
//!
//! What came after a Resize and is about what it changes waits for it, as upstream's main thread
//! took it behind the resize (PITFALLS 145 and 160):
//! - the pane's own requests ([`Resizes::after`]): its input (keys, the mouse, pastes, writes)
//!   and what reads what it shows (the window's poll, its fetches of rows, its dimensions), each
//!   run once the pane's resizes are over, in the order they came. Taken at once, they overtook
//!   the resize: the poll and the fetch a window sends straight after its Resize were answered
//!   before the resize thread had the terminal, so the window heard of the pane's output twice,
//!   for a frame more, and now and then took the pane's old size from the poll's answer;
//! - what lists or changes a tab's layout ([`Resizes::after_all`]): ListPanes, which lists every
//!   pane's size, and a split, a zoom, a pane killed or moved and a split's size adjusted, which
//!   resize panes on the main thread. These wait until no pane is being resized at all. Answered
//!   at once, ListPanes listed panes at sizes their threads hadn't reached, and blocked the main
//!   thread on the terminal of each pane a thread was rewrapping; a window that sends back the
//!   sizes it's listed (any build before perf/mux-client) sent those older sizes as new Resizes,
//!   the server said TabResized for them, the window listed again, and a drag of its frame never
//!   ended: 11,446 Resize in 108 s and 2.8 cores of the server (PITFALLS 159 and 160). And a
//!   pane resized on the main thread while its own thread resized it could be left with its
//!   pseudoconsole and its terminal at different sizes, as two connections could (cbb9c5f60).
//!
//! No other pane's requests wait. What the main thread does with a pane being resized on its own
//! account (a push of its output) still waits for its terminal's lock, as it waited behind the
//! resize before (FLOWS.md, "A window's resize on a mux domain", row 7). So does the main
//! thread's prune of a pane whose program ended beside one being resized, which resizes the one
//! left on the main thread outside the book (rare: what's left of the race above).

use anyhow::anyhow;
use codec::{Pdu, UnitResponse};
use mux::pane::PaneId;
use mux::renderable::RenderableDimensions;
use mux::tab::TabId;
use mux::Mux;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use wezterm_term::TerminalSize;

/// How a request is answered: once, with what happened
pub type Answer = Box<dyn FnOnce(anyhow::Result<Pdu>) + Send>;

/// A request that waits for resizes: run once, when they're over
pub type After = Box<dyn FnOnce() + Send>;

/// A connection, by a number of its own for as long as the server runs ([`new_conn`]): whose a
/// Resize is, so its own layout isn't told back to it
pub type Conn = u64;

/// A number for a new connection, never one another has had
pub fn new_conn() -> Conn {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

thread_local! {
    /// On the main thread, while a tab's split sizes are rebuilt after resizes that one
    /// connection alone asked for: that tab and connection
    static OWN: std::cell::Cell<Option<(TabId, Conn)>> = const { std::cell::Cell::new(None) };
}

/// Whether a TabResized for `tab_id`, said now, comes of `conn`'s own Resizes alone, so `conn`
/// isn't told: its subscriber asks, as the rebuild's notification reaches it
pub fn is_own(tab_id: TabId, conn: Conn) -> bool {
    OWN.with(|own| own.get() == Some((tab_id, conn)))
}

/// `rebuild` run with its TabResized taken as `asker`'s own (when one connection alone asked)
fn rebuilding_as(tab_id: TabId, asker: Option<Conn>, rebuild: impl FnOnce()) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            OWN.with(|own| own.set(None));
        }
    }
    let _reset = Reset;
    OWN.with(|own| own.set(asker.map(|conn| (tab_id, conn))));
    rebuild()
}

/// A pane's resizes in progress
#[derive(Default)]
struct PaneResize {
    /// The tab of the resize running, as its request named it
    tab: TabId,
    /// The connection whose size the resize running applies
    conn: Conn,
    /// The newest size asked for it, its tab and the connection that asked, not started yet
    asked: Option<(TabId, TerminalSize, Conn)>,
    /// A thread is resizing it
    running: bool,
    /// The requests to answer once it has the newest size asked
    waiting: Vec<Answer>,
    /// The pane's other requests that came since, to run then, in the order they came
    after: Vec<After>,
}

/// The bookkeeping of the resizes, by pane, apart from the panes and the threads
#[derive(Default)]
struct Book {
    panes: HashMap<PaneId, PaneResize>,
    /// The tabs a pane of which was resized since their split sizes were last rebuilt, each with
    /// the one connection whose sizes those were (None: more than one's)
    resized_tabs: HashMap<TabId, Option<Conn>>,
    /// The requests that wait for every resize under way, in the order they came
    all: Vec<After>,
}

/// What follows the end of one of a pane's resizes
enum Next {
    /// The newest size asked since: started next (the book has its tab)
    Start(TerminalSize),
    /// The pane's resizes are over
    Over(Over),
}

/// A pane's resizes over: what the main thread does then, in this order
#[derive(Default)]
struct Over {
    /// The tabs whose split sizes to rebuild: a pane of theirs was resized, and none is being
    /// resized now. Each with the one connection whose sizes those were, which isn't told
    rebuild: Vec<(TabId, Option<Conn>)>,
    /// The pane's Resize requests, to answer
    waiting: Vec<Answer>,
    /// The pane's other requests that waited, in the order they came
    after: Vec<After>,
    /// The requests that waited for every resize, when none is under way now
    all: Vec<After>,
}

impl Book {
    /// A request for `size`: the size to resize the pane to now, or None when it's being resized
    /// and the request waits for that and the newest size asked after it
    fn ask(
        &mut self,
        pane_id: PaneId,
        tab_id: TabId,
        size: TerminalSize,
        conn: Conn,
        answer: Answer,
    ) -> Option<(TabId, TerminalSize)> {
        let pane = self.panes.entry(pane_id).or_default();
        pane.waiting.push(answer);
        if pane.running {
            pane.asked = Some((tab_id, size, conn));
            None
        } else {
            pane.running = true;
            pane.tab = tab_id;
            pane.conn = conn;
            Some((tab_id, size))
        }
    }

    /// A request about the pane other than a resize: kept for when its resizes are over, or given
    /// back to run now when none is under way
    fn after(&mut self, pane_id: PaneId, then: After) -> Option<After> {
        match self.panes.get_mut(&pane_id) {
            Some(pane) => {
                pane.after.push(then);
                None
            }
            None => Some(then),
        }
    }

    /// A request about every pane's size or a tab's layout: kept for when no pane is being
    /// resized, or given back to run now when none is
    fn after_all(&mut self, then: After) -> Option<After> {
        if self.panes.is_empty() {
            Some(then)
        } else {
            self.all.push(then);
            None
        }
    }

    /// One of the pane's resizes is over (`resized`: the pane was resized, and its tab's split
    /// sizes need rebuilding): the newest size asked since, to start next, or else what's to be
    /// done now that its resizes are over, the pane no longer being resized
    fn finished(&mut self, pane_id: PaneId, resized: bool) -> Next {
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            if resized {
                let mine = Some(pane.conn);
                self.resized_tabs
                    .entry(pane.tab)
                    .and_modify(|asker| {
                        if *asker != mine {
                            *asker = None;
                        }
                    })
                    .or_insert(mine);
            }
            if let Some((tab, size, conn)) = pane.asked.take() {
                pane.tab = tab;
                pane.conn = conn;
                return Next::Start(size);
            }
        }
        let pane = self.panes.remove(&pane_id).unwrap_or_default();
        // A tab is rebuilt once none of its panes is being resized or asked to be
        let busy: HashSet<TabId> = self
            .panes
            .values()
            .flat_map(|p| std::iter::once(p.tab).chain(p.asked.map(|(tab, _, _)| tab)))
            .collect();
        let rebuild: Vec<(TabId, Option<Conn>)> = self
            .resized_tabs
            .iter()
            .filter(|(tab, _)| !busy.contains(tab))
            .map(|(tab, asker)| (*tab, *asker))
            .collect();
        for (tab, _) in &rebuild {
            self.resized_tabs.remove(tab);
        }
        let all = if self.panes.is_empty() {
            std::mem::take(&mut self.all)
        } else {
            vec![]
        };
        Next::Over(Over {
            rebuild,
            waiting: pane.waiting,
            after: pane.after,
            all,
        })
    }
}

/// The server's resizes, every connection's: a pane's pseudoconsole and terminal are resized
/// under separate locks, so two resizes of one pane at once could leave them at different sizes
pub fn resizes() -> &'static Resizes {
    static RESIZES: std::sync::OnceLock<Resizes> = std::sync::OnceLock::new();
    RESIZES.get_or_init(Resizes::default)
}

/// The resizes asked and under way, by pane
#[derive(Clone, Default)]
pub struct Resizes {
    book: Arc<Mutex<Book>>,
}

impl Resizes {
    /// A request from `conn`, read on the main thread: `size` for `pane_id` in `tab_id`,
    /// answered with `answer` once the pane has it or a newer size
    pub fn ask(
        &self,
        pane_id: PaneId,
        tab_id: TabId,
        size: TerminalSize,
        conn: Conn,
        answer: Answer,
    ) {
        let now = self.book.lock().unwrap().ask(pane_id, tab_id, size, conn, answer);
        if let Some((_, size)) = now {
            self.start(pane_id, size);
        }
    }

    /// Any thread: `then`, a request about `pane_id` that isn't a resize, run now, or once the
    /// pane's resizes are over when it's being resized, after the requests about it that came
    /// before it
    pub fn after(&self, pane_id: PaneId, then: After) {
        let now = self.book.lock().unwrap().after(pane_id, then);
        if let Some(then) = now {
            then();
        }
    }

    /// Any thread: `then`, a request that lists every pane's size or changes a tab's layout
    /// (ListPanes, a split, a zoom, a pane killed or moved, a split's size adjusted), run now, or
    /// once no pane is being resized, after the requests of that kind that came before it
    pub fn after_all(&self, then: After) {
        let now = self.book.lock().unwrap().after_all(then);
        if let Some(then) = now {
            then();
        }
    }

    /// On the main thread: the pane resized to `size` on a thread of its own, unless it has that
    /// size already; then [`Resizes::done`], on the main thread
    fn start(&self, pane_id: PaneId, size: TerminalSize) {
        let pane = match Mux::get().get_pane(pane_id) {
            Some(pane) => pane,
            None => {
                let gone = Err(anyhow!("no such pane {}", pane_id));
                return self.done(pane_id, gone, false);
            }
        };
        if is_size(&pane.get_dimensions(), &size) {
            return self.done(pane_id, Ok(()), false);
        }
        let me = self.clone();
        let run = std::thread::Builder::new()
            .name("lbterm-resize".into())
            .spawn(move || {
                let result = guarded(pane_id, &size, || pane.resize(size));
                promise::spawn::spawn_into_main_thread(async move {
                    me.done(pane_id, result, true);
                })
                .detach();
            });
        if let Err(err) = run {
            let none = Err(anyhow!("no thread to resize pane {}: {:#}", pane_id, err));
            self.done(pane_id, none, false);
        }
    }

    /// On the main thread: the pane's last resize is over with `result` (`resized`: the pane was
    /// resized). The newest size asked since is started, or, with none, the tabs none of whose
    /// panes is being resized now have their split sizes rebuilt (a TabResized it says going to
    /// every connection but the one whose sizes alone those were), every Resize waiting is
    /// answered, the pane's other requests run, and, with no pane being resized, the requests
    /// that waited for them all
    fn done(&self, pane_id: PaneId, result: anyhow::Result<()>, resized: bool) {
        if let Err(err) = &result {
            log::warn!("resizing pane {}: {:#}", pane_id, err);
        }
        let next = self
            .book
            .lock()
            .unwrap()
            .finished(pane_id, resized && result.is_ok());
        match next {
            Next::Start(size) => self.start(pane_id, size),
            Next::Over(over) => {
                let mux = Mux::get();
                for (tab_id, asker) in over.rebuild {
                    match mux.get_tab(tab_id) {
                        Some(tab) => rebuilding_as(tab_id, asker, || {
                            tab.rebuild_splits_sizes_from_contained_panes()
                        }),
                        None => log::debug!("resized a pane of tab {}, gone since", tab_id),
                    }
                }
                for answer in over.waiting {
                    answer(match &result {
                        Ok(()) => Ok(Pdu::UnitResponse(UnitResponse {})),
                        Err(err) => Err(anyhow!("{:#}", err)),
                    });
                }
                for then in over.after {
                    then();
                }
                for then in over.all {
                    then();
                }
            }
        }
    }
}

/// `resize`, run on a pane's resize thread, with a panic in it turned into an error that names
/// the pane: the book has to hear that the resize is over however it ended, or the pane would be
/// marked as being resized for good and every request behind it would wait, unanswered and
/// unsaid (PITFALLS 161). The panic hook has logged it with its backtrace by then
fn guarded(
    pane_id: PaneId,
    size: &TerminalSize,
    resize: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(resize)).unwrap_or_else(|panic| {
        let said = match panic.downcast_ref::<&str>() {
            Some(s) => s.to_string(),
            None => match panic.downcast_ref::<String>() {
                Some(s) => s.clone(),
                None => "no message".to_string(),
            },
        };
        Err(anyhow!(
            "resizing pane {} to {}x{} panicked: {}",
            pane_id,
            size.cols,
            size.rows,
            said
        ))
    })
}

/// Whether a pane's dimensions are `size` already: its columns, rows, pixels and dpi
pub fn is_size(dims: &RenderableDimensions, size: &TerminalSize) -> bool {
    dims.cols == size.cols
        && dims.viewport_rows == size.rows
        && dims.pixel_width == size.pixel_width
        && dims.pixel_height == size.pixel_height
        && dims.dpi == size.dpi
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(cols: usize) -> TerminalSize {
        TerminalSize {
            rows: 50,
            cols,
            pixel_width: cols * 9,
            pixel_height: 50 * 20,
            dpi: 96,
        }
    }

    /// An answer that says its request's number into `said` when it's given
    fn answer(said: &Arc<Mutex<Vec<usize>>>, n: usize) -> Answer {
        let said = Arc::clone(said);
        Box::new(move |result| {
            assert!(result.is_ok());
            said.lock().unwrap().push(n);
        })
    }

    /// A request that says its number into `said` when it's run
    fn then(said: &Arc<Mutex<Vec<usize>>>, n: usize) -> After {
        let said = Arc::clone(said);
        Box::new(move || said.lock().unwrap().push(n))
    }

    fn started(next: Next) -> Option<TerminalSize> {
        match next {
            Next::Start(size) => Some(size),
            Next::Over(_) => None,
        }
    }

    fn over(next: Next) -> Over {
        match next {
            Next::Start(_) => panic!("another size started, where the pane's resizes were over"),
            Next::Over(over) => over,
        }
    }

    /// What's over, run as `done` runs it: the Resize answers, the pane's requests, then those
    /// that waited for every resize
    fn run(over: Over) {
        for answer in over.waiting {
            answer(Ok(Pdu::UnitResponse(UnitResponse {})));
        }
        for then in over.after {
            then();
        }
        for then in over.all {
            then();
        }
    }

    #[test]
    fn a_pane_being_resized_takes_the_newest_size_after_and_answers_all() {
        let said = Arc::new(Mutex::new(vec![]));
        let mut book = Book::default();
        assert_eq!(book.ask(1, 10, size(100), 1, answer(&said, 1)), Some((10, size(100))), "started");
        // Three more while it runs: none starts
        assert_eq!(book.ask(1, 10, size(99), 1, answer(&said, 2)), None);
        assert_eq!(book.ask(1, 10, size(98), 1, answer(&said, 3)), None);
        assert_eq!(book.ask(1, 10, size(97), 1, answer(&said, 4)), None);
        // Its first resize over: the newest asked is next, once, and nobody's answered yet
        assert_eq!(started(book.finished(1, true)), Some(size(97)));
        assert_eq!(book.panes[&1].tab, 10);
        assert!(said.lock().unwrap().is_empty());
        // That one over: everyone's answered
        let done = over(book.finished(1, true));
        assert_eq!(done.waiting.len(), 4);
        assert!(done.after.is_empty());
        assert_eq!(done.rebuild, vec![(10, Some(1))], "its tab rebuilt once, at the end");
        run(done);
        assert_eq!(*said.lock().unwrap(), vec![1, 2, 3, 4]);
        // and the next request starts at once again
        assert!(book.ask(1, 10, size(96), 1, answer(&said, 5)).is_some());
    }

    #[test]
    fn panes_are_resized_side_by_side() {
        let said = Arc::new(Mutex::new(vec![]));
        let mut book = Book::default();
        for pane in 1..=12 {
            assert!(
                book.ask(pane, pane + 100, size(90), 1, answer(&said, pane)).is_some(),
                "pane {} starts though the others run",
                pane
            );
        }
        for pane in (1..=12).rev() {
            let done = over(book.finished(pane, true));
            assert_eq!(done.waiting.len(), 1);
            assert_eq!(done.rebuild, vec![(pane + 100, Some(1))], "a tab of its own, rebuilt as it ends");
        }
    }

    #[test]
    fn a_panes_other_requests_wait_for_its_resizes_in_the_order_they_came() {
        let said = Arc::new(Mutex::new(vec![]));
        let mut book = Book::default();
        // No resize under way: a request is given back to run at once
        let now = book.after(1, then(&said, 1)).expect("run now");
        now();
        assert_eq!(*said.lock().unwrap(), vec![1]);
        // Pane 1 being resized: its poll, a key and a fetch wait, in order, through a newer size
        assert!(book.ask(1, 10, size(100), 1, answer(&said, 100)).is_some());
        assert!(book.after(1, then(&said, 2)).is_none(), "the poll waits");
        assert!(book.after(1, then(&said, 3)).is_none(), "the key waits");
        assert!(book.ask(1, 10, size(99), 1, answer(&said, 101)).is_none());
        assert!(book.after(1, then(&said, 4)).is_none(), "the fetch waits");
        // Another pane's request doesn't wait
        assert!(book.after(2, then(&said, 5)).is_some(), "pane 2 isn't being resized");
        // The first resize over: the newer size next, nothing run yet
        assert!(started(book.finished(1, true)).is_some());
        assert_eq!(*said.lock().unwrap(), vec![1]);
        // All over: the Resize requests answered, then the others in the order they came
        run(over(book.finished(1, true)));
        assert_eq!(*said.lock().unwrap(), vec![1, 100, 101, 2, 3, 4]);
        // and the pane's next request runs at once again
        assert!(book.after(1, then(&said, 6)).is_some());
    }

    /// A listing of every pane's size waits for every resize under way, so it never lists a pane
    /// at a size its thread hasn't reached (an older window sends a listed size back as a Resize)
    #[test]
    fn a_listing_waits_for_every_resize() {
        let said = Arc::new(Mutex::new(vec![]));
        let mut book = Book::default();
        // Nothing being resized: it runs at once
        let now = book.after_all(then(&said, 1)).expect("run now");
        now();
        // Two panes of two tabs being resized: the listing waits for both
        assert!(book.ask(1, 10, size(100), 1, answer(&said, 100)).is_some());
        assert!(book.ask(2, 20, size(100), 1, answer(&said, 200)).is_some());
        assert!(book.after_all(then(&said, 2)).is_none(), "ListPanes waits");
        assert!(book.after(1, then(&said, 3)).is_none(), "pane 1's key waits for pane 1");
        assert!(book.after(3, then(&said, 4)).is_some(), "pane 3, not being resized, doesn't");
        // Pane 1 over: its own requests, not the listing, which pane 2 still holds
        let first = over(book.finished(1, true));
        assert!(first.all.is_empty(), "pane 2 is still being resized");
        run(first);
        assert_eq!(*said.lock().unwrap(), vec![1, 100, 3]);
        // A newer size for pane 2 while the listing waits: it waits for that one too
        assert!(book.ask(2, 20, size(99), 1, answer(&said, 201)).is_none());
        assert!(started(book.finished(2, true)).is_some());
        // Pane 2 over: its answers, then the listing, at the sizes every pane ended at
        run(over(book.finished(2, true)));
        assert_eq!(*said.lock().unwrap(), vec![1, 100, 3, 200, 201, 2]);
        // and the next listing runs at once again
        assert!(book.after_all(then(&said, 5)).is_some());
    }

    /// A tab's split sizes are rebuilt once none of its panes is being resized: rebuilt as the
    /// first ended, the main thread would wait on the second's terminal, which its thread holds
    #[test]
    fn a_tab_is_rebuilt_once_its_last_pane_is_resized() {
        let said = Arc::new(Mutex::new(vec![]));
        let mut book = Book::default();
        // Two panes of tab 10, one of tab 20
        assert!(book.ask(1, 10, size(60), 1, answer(&said, 1)).is_some());
        assert!(book.ask(2, 10, size(59), 1, answer(&said, 2)).is_some());
        assert!(book.ask(3, 20, size(120), 1, answer(&said, 3)).is_some());
        let first = over(book.finished(1, true));
        assert!(first.rebuild.is_empty(), "pane 2 of the same tab is still being resized");
        assert_eq!(first.waiting.len(), 1, "pane 1's answer doesn't wait for pane 2");
        // Pane 2 had the size already (nothing resized), but pane 1 was: tab 10 is rebuilt now
        assert_eq!(over(book.finished(2, false)).rebuild, vec![(10, Some(1))]);
        // Pane 3 has a newer size asked: tab 20 waits for it
        assert!(book.ask(3, 20, size(119), 1, answer(&said, 4)).is_none());
        assert!(started(book.finished(3, true)).is_some());
        assert_eq!(over(book.finished(3, true)).rebuild, vec![(20, Some(1))]);
        // A pane that had its size already and whose tab nothing resized: nothing rebuilt
        assert!(book.ask(4, 30, size(80), 1, answer(&said, 5)).is_some());
        assert!(over(book.finished(4, false)).rebuild.is_empty());
    }

    /// A resize that panics on its thread is an error the book hears like any other: the pane
    /// leaves the book, its Resize requests are answered and the requests behind it run
    #[test]
    fn a_resize_that_panics_still_ends() {
        let said = Arc::new(Mutex::new(vec![]));
        let result = guarded(7, &size(80), || panic!("rewrapping line {}", 12));
        let err = format!("{:#}", result.expect_err("a panic is an error"));
        assert!(err.contains("pane 7") && err.contains("80x50"), "names the pane: {}", err);
        assert!(err.contains("rewrapping line 12"), "says the panic: {}", err);
        assert!(guarded(7, &size(80), || Ok(())).is_ok());

        let mut book = Book::default();
        let heard = Arc::clone(&said);
        let told: Answer = Box::new(move |result| {
            assert!(result.is_err(), "the Resize request hears the error");
            heard.lock().unwrap().push(1);
        });
        assert!(book.ask(7, 70, size(80), 1, told).is_some());
        assert!(book.after(7, then(&said, 2)).is_none(), "a key waits for the resize");
        assert!(book.after_all(then(&said, 3)).is_none(), "ListPanes waits for it");
        // done() gives the book the end with nothing resized (`resized && result.is_ok()`)
        let done = over(book.finished(7, false));
        assert!(done.rebuild.is_empty());
        assert_eq!(done.waiting.len(), 1);
        for answer in done.waiting {
            answer(Err(anyhow!("{:#}", err)));
        }
        for then in done.after.into_iter().chain(done.all) {
            then();
        }
        assert_eq!(*said.lock().unwrap(), vec![1, 2, 3]);
        assert!(book.panes.is_empty(), "the pane isn't being resized any more");
        assert!(book.after(7, then(&said, 4)).is_some(), "its next key runs at once");
    }

    /// A layout moved by one connection's Resizes alone is that connection's own, which isn't
    /// told it: a window that resyncs on it sends back what it's listed (PITFALLS 159)
    #[test]
    fn a_rebuild_is_the_connections_whose_sizes_alone_it_applied() {
        let said = Arc::new(Mutex::new(vec![]));
        let mut book = Book::default();
        // Connection 1 resizes tab 10's two panes, through a newer size
        assert!(book.ask(1, 10, size(60), 1, answer(&said, 1)).is_some());
        assert!(book.ask(2, 10, size(59), 1, answer(&said, 2)).is_some());
        assert!(book.ask(1, 10, size(61), 1, answer(&said, 3)).is_none());
        assert!(started(book.finished(1, true)).is_some());
        assert!(over(book.finished(1, true)).rebuild.is_empty());
        assert_eq!(over(book.finished(2, true)).rebuild, vec![(10, Some(1))]);
        // Connection 2 asks a newer size of a pane connection 1 is resizing: the tab is everyone's
        assert!(book.ask(3, 20, size(80), 1, answer(&said, 4)).is_some());
        assert!(book.ask(3, 20, size(81), 2, answer(&said, 5)).is_none());
        assert!(started(book.finished(3, true)).is_some());
        assert_eq!(over(book.finished(3, true)).rebuild, vec![(20, None)]);
        // Two panes of one tab, one each: everyone's too
        assert!(book.ask(4, 30, size(70), 1, answer(&said, 6)).is_some());
        assert!(book.ask(5, 30, size(70), 2, answer(&said, 7)).is_some());
        assert!(over(book.finished(4, true)).rebuild.is_empty());
        assert_eq!(over(book.finished(5, true)).rebuild, vec![(30, None)]);
        // A pane that had its size already resized nothing: nothing of its connection's
        assert!(book.ask(6, 40, size(70), 3, answer(&said, 8)).is_some());
        assert!(over(book.finished(6, false)).rebuild.is_empty());
    }

    /// While a rebuild is one connection's own, a TabResized for that tab isn't that
    /// connection's news, and is everyone else's; after it, it's everyone's again
    #[test]
    fn a_tabresized_is_kept_from_the_connection_that_asked_only_during_its_rebuild() {
        assert!(!is_own(10, 1));
        rebuilding_as(10, Some(1), || {
            assert!(is_own(10, 1), "connection 1's own");
            assert!(!is_own(10, 2), "connection 2 is told");
            assert!(!is_own(11, 1), "another tab is news to connection 1");
        });
        assert!(!is_own(10, 1), "after the rebuild");
        rebuilding_as(10, None, || assert!(!is_own(10, 1), "more than one's: everyone's told"));
        let r = std::panic::catch_unwind(|| rebuilding_as(10, Some(1), || panic!("in a rebuild")));
        assert!(r.is_err());
        assert!(!is_own(10, 1), "put back when it panics");
        assert_ne!(new_conn(), new_conn(), "each connection a number of its own");
    }

    #[test]
    fn a_size_is_the_panes_only_when_everything_matches() {
        let mut dims = RenderableDimensions {
            cols: 120,
            viewport_rows: 50,
            scrollback_rows: 5000,
            physical_top: 4950,
            scrollback_top: 0,
            dpi: 96,
            pixel_width: 1080,
            pixel_height: 1000,
            reverse_video: false,
        };
        assert!(is_size(&dims, &size(120)));
        assert!(!is_size(&dims, &size(119)));
        dims.dpi = 144;
        assert!(!is_size(&dims, &size(120)), "another dpi");
        dims.dpi = 96;
        dims.pixel_width = 1200;
        assert!(!is_size(&dims, &size(120)), "the same cells at another font size");
    }
}
