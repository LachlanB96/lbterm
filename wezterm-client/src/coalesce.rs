//! LBTerm: an async job that runs one at a time, its callers sharing runs (ClientDomain::resync)
use parking_lot::Mutex;
use std::future::Future;

/// Runs an async job one at a time. A caller that comes while it runs waits for one more run,
/// which starts after it asked, so it sees everything that had happened by then, and every
/// caller that comes during the same run shares that one. ClientDomain::resync uses it:
/// upstream sent a ListPanes for every PDU about a pane the GUI didn't know yet and for every
/// TabResized and TabAddedToWindow, so a new tab brought two to four whole resyncs at once,
/// each syncing every tab.
#[derive(Default)]
pub struct Coalesced {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    running: bool,
    /// asked for while it ran: one more run, then the waiters are told
    again: bool,
    waiters: Vec<smol::channel::Sender<Result<(), String>>>,
}

/// The caller doing the runs: if its future is dropped before it finishes, the next caller
/// can run, and those waiting on it are told
struct Leader<'a> {
    state: &'a Mutex<State>,
    done: bool,
}

impl Drop for Leader<'_> {
    fn drop(&mut self) {
        if !self.done {
            let mut state = self.state.lock();
            state.running = false;
            state.again = false;
            for tx in std::mem::take(&mut state.waiters) {
                let _ = tx.try_send(Err("the run it waited for was dropped".to_string()));
            }
        }
    }
}

impl Coalesced {
    pub async fn run<F, Fut>(&self, mut job: F) -> anyhow::Result<()>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = anyhow::Result<()>>,
    {
        let wait = {
            let mut state = self.state.lock();
            if state.running {
                state.again = true;
                let (tx, rx) = smol::channel::bounded(1);
                state.waiters.push(tx);
                Some(rx)
            } else {
                state.running = true;
                None
            }
        };
        if let Some(rx) = wait {
            return match rx.recv().await {
                Ok(result) => result.map_err(|e| anyhow::anyhow!(e)),
                Err(_) => Err(anyhow::anyhow!("the run it waited for was dropped")),
            };
        }

        let mut leader = Leader {
            state: &self.state,
            done: false,
        };
        loop {
            let result = job().await;
            let waiters = {
                let mut state = self.state.lock();
                if result.is_ok() && state.again {
                    state.again = false;
                    None
                } else {
                    state.running = false;
                    state.again = false;
                    Some(std::mem::take(&mut state.waiters))
                }
            };
            if let Some(waiters) = waiters {
                leader.done = true;
                let shared = result.as_ref().map(|_| ()).map_err(|e| format!("{e:#}"));
                for tx in waiters {
                    let _ = tx.try_send(shared.clone());
                }
                return result;
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::FutureExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn callers_during_a_run_share_one_more_run() {
        let flight = Coalesced::default();
        let runs = AtomicUsize::new(0);
        let (gate_tx, gate_rx) = smol::channel::bounded::<()>(1);
        // each run waits at the gate, so the other callers come while it runs
        let job = || {
            let (runs, gate_rx) = (&runs, gate_rx.clone());
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
                gate_rx.recv().await.ok();
                Ok(())
            }
        };
        let (a, b, c, ()) = smol::block_on(async {
            futures::join!(flight.run(job), flight.run(job), flight.run(job), async {
                gate_tx.send(()).await.unwrap();
                gate_tx.send(()).await.unwrap();
            })
        });
        assert!(a.is_ok() && b.is_ok() && c.is_ok());
        assert_eq!(runs.load(Ordering::SeqCst), 2, "the first caller's run, and one more for the two that came during it");
    }

    #[test]
    fn a_caller_after_a_run_runs_again() {
        let flight = Coalesced::default();
        let runs = AtomicUsize::new(0);
        let job = || {
            let runs = &runs;
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        };
        smol::block_on(flight.run(job)).unwrap();
        smol::block_on(flight.run(job)).unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_failed_run_fails_the_callers_waiting_on_it() {
        let flight = Coalesced::default();
        let (gate_tx, gate_rx) = smol::channel::bounded::<()>(1);
        let job = || {
            let gate_rx = gate_rx.clone();
            async move {
                gate_rx.recv().await.ok();
                Err(anyhow::anyhow!("no server"))
            }
        };
        let (a, b, ()) = smol::block_on(async {
            futures::join!(flight.run(job), flight.run(job), async {
                gate_tx.send(()).await.unwrap();
            })
        });
        assert_eq!(a.unwrap_err().to_string(), "no server");
        assert_eq!(b.unwrap_err().to_string(), "no server");
    }

    #[test]
    fn a_dropped_run_lets_the_next_caller_run() {
        let flight = Coalesced::default();
        let (_gate_tx, gate_rx) = smol::channel::bounded::<()>(1);
        // a run stuck at its gate, dropped after its first poll
        let stuck = flight.run(|| {
            let gate_rx = gate_rx.clone();
            async move {
                gate_rx.recv().await.ok();
                Ok(())
            }
        });
        assert!(stuck.now_or_never().is_none());
        assert!(smol::block_on(flight.run(|| async { Ok(()) })).is_ok());
    }
}
