//! LBTerm: upstream's metrics handles found without the recorder's lock (lbterm/PERF.md, "What
//! upstream's metrics cost"). In metrics 0.23 every `histogram!` and `counter!` asks the recorder
//! for its handle again, and upstream's recorder (wezterm-gui/src/stats.rs) took its one global
//! lock, hashed the key's name and labels with SipHash for a HashMap and cloned an Arc on every
//! call: a few times a line each frame, a glyph, a shaped run, a main-thread task and a PDU, on the
//! GUI thread and the mux client's thread alike. A thread now keeps each handle it's been given,
//! by the key's own hash (worked out once for a key with a fixed name and kept in its static), and
//! asks the recorder only for a key it hasn't seen.

use metrics::Key;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// A key's hash is already a hash: the map takes it as it is
#[derive(Default)]
struct AsIs(u64);

impl Hasher for AsIs {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = self.0.rotate_left(8) ^ u64::from(*b);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }
}

/// The handles one thread has been given, each under its key, so a key seen before is a lookup
/// in the thread's own map, with no lock
pub struct Handles<V> {
    by_hash: HashMap<u64, Vec<(Key, V)>, BuildHasherDefault<AsIs>>,
    hash: fn(&Key) -> u64,
}

impl<V: Clone> Handles<V> {
    pub fn new() -> Self {
        Self::hashed_by(Key::get_hash)
    }

    fn hashed_by(hash: fn(&Key) -> u64) -> Self {
        Self {
            by_hash: HashMap::default(),
            hash,
        }
    }

    /// The handle for `key`: the one kept, or `register`'s, kept from now on
    #[inline]
    pub fn get(&mut self, key: &Key, register: impl FnOnce() -> V) -> V {
        let hash = (self.hash)(key);
        if let Some(kept) = self.by_hash.get(&hash) {
            if let Some((_, handle)) = kept.iter().find(|(k, _)| k == key) {
                return handle.clone();
            }
        }
        let handle = register();
        self.by_hash
            .entry(hash)
            .or_default()
            .push((key.clone(), handle.clone()));
        handle
    }

    /// How many keys it holds
    pub fn len(&self) -> usize {
        self.by_hash.values().map(Vec::len).sum()
    }
}

impl<V: Clone> Default for Handles<V> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metrics::{
        Counter, CounterFn, Gauge, Histogram, HistogramFn, KeyName, Metadata, Recorder,
        SharedString, Unit,
    };
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    use std::sync::{Arc, Mutex};

    #[test]
    fn a_key_seen_before_is_its_handle_without_registering_again() {
        let mut handles = Handles::<u32>::new();
        let mut registered = 0;
        let mut get = |handles: &mut Handles<u32>, key: &Key, value: u32| {
            handles.get(key, || {
                registered += 1;
                value
            })
        };
        static PAINT: Key = Key::from_static_name("gui.paint.impl");
        assert_eq!(get(&mut handles, &PAINT, 1), 1);
        assert_eq!(get(&mut handles, &PAINT, 2), 1, "kept");
        // A name made at run time, as lfucache's were, and labels built for each call, as
        // codec's are, find theirs too
        let hit: &'static str = "line_quad_cache.hit.rate";
        assert_eq!(get(&mut handles, &Key::from_name(hit), 3), 3);
        assert_eq!(get(&mut handles, &Key::from_name(hit), 4), 3);
        let pdu = |name: &'static str| {
            Key::from_parts("pdu.size", vec![metrics::Label::new("pdu", name)])
        };
        assert_eq!(get(&mut handles, &pdu("GetLines"), 5), 5);
        assert_eq!(get(&mut handles, &pdu("GetLines"), 6), 5);
        assert_eq!(
            get(&mut handles, &pdu("Pong"), 7),
            7,
            "another label is another key"
        );
        assert_eq!(handles.len(), 4);
        drop(get);
        assert_eq!(registered, 4);
    }

    #[test]
    fn keys_whose_hashes_collide_keep_their_own() {
        let mut handles = Handles::<&'static str>::hashed_by(|_| 7);
        let a = Key::from_name("a");
        let b = Key::from_name("b");
        assert_eq!(handles.get(&a, || "a"), "a");
        assert_eq!(handles.get(&b, || "b"), "b");
        assert_eq!(handles.get(&a, || "wrong"), "a");
        assert_eq!(handles.get(&b, || "wrong"), "b");
        assert_eq!(handles.len(), 2);
    }

    /// What a histogram's record does here: the same for both recorders, so the two differ only
    /// in finding the handle
    struct Count(AtomicU64);
    impl HistogramFn for Count {
        fn record(&self, _value: f64) {
            self.0.fetch_add(1, Relaxed);
        }
    }
    impl CounterFn for Count {
        fn increment(&self, n: u64) {
            self.0.fetch_add(n, Relaxed);
        }
        fn absolute(&self, n: u64) {
            self.0.store(n, Relaxed);
        }
    }

    /// Upstream's way (stats.rs before LBTerm): one lock for all keys, a SipHash HashMap
    #[derive(Default)]
    struct Upstream(Mutex<HashMap<Key, Arc<Count>>>);

    impl Upstream {
        fn handle(&self, key: &Key) -> Arc<Count> {
            let mut map = self.0.lock().unwrap();
            match map.get(key) {
                Some(existing) => existing.clone(),
                None => {
                    let h = Arc::new(Count(AtomicU64::new(0)));
                    map.insert(key.clone(), h.clone());
                    h
                }
            }
        }
    }

    thread_local! {
        static KEPT: RefCell<Handles<Histogram>> = RefCell::new(Handles::new());
    }

    /// LBTerm's: the thread's own handles first
    #[derive(Default)]
    struct Kept(Upstream);

    macro_rules! recorder {
        ($t:ty, $histogram:expr) => {
            impl Recorder for $t {
                fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
                fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
                fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
                fn register_counter(&self, key: &Key, _: &Metadata) -> Counter {
                    Counter::from_arc(self.upstream().handle(key))
                }
                fn register_gauge(&self, _: &Key, _: &Metadata) -> Gauge {
                    Gauge::noop()
                }
                fn register_histogram(&self, key: &Key, _: &Metadata) -> Histogram {
                    let f: fn(&Self, &Key) -> Histogram = $histogram;
                    f(self, key)
                }
            }
        };
    }
    impl Upstream {
        fn upstream(&self) -> &Upstream {
            self
        }
    }
    impl Kept {
        fn upstream(&self) -> &Upstream {
            &self.0
        }
    }
    recorder!(Upstream, |r, key| Histogram::from_arc(r.handle(key)));
    recorder!(Kept, |r, key| KEPT.with(|kept| {
        kept.borrow_mut()
            .get(key, || Histogram::from_arc(r.0.handle(key)))
    }));

    /// What one frame's records of a window of 60 lines come to, each as the GUI makes it: per
    /// line a literal key three times (quad_buffer_apply and, before LBTerm counted them, two
    /// caches' hits, which are named at run time), per frame gui.paint.impl and paint_pane.lines,
    /// and a PDU's label built for its call (codec's)
    fn frame() {
        let line_quad: &'static str = "line_quad_cache.hit.rate";
        let line_state: &'static str = "line_state_cache.hit.rate";
        for _ in 0..60 {
            metrics::histogram!(line_state).record(1.);
            metrics::histogram!(line_quad).record(1.);
            metrics::histogram!("quad_buffer_apply").record(0.000_001);
        }
        metrics::histogram!("paint_pane.lines").record(0.001);
        metrics::histogram!("gui.paint.impl").record(0.002);
        metrics::histogram!("pdu.size", "pdu" => stringify!(GetPaneRenderChanges)).record(300.);
    }

    /// ns a record, over `n` frames, with `recorder` (None: no recorder at all)
    fn per_record(recorder: Option<&dyn Recorder>, n: usize) -> f64 {
        let run = || {
            let t = std::time::Instant::now();
            for _ in 0..n {
                frame();
            }
            t.elapsed().as_nanos() as f64 / (n * 183) as f64
        };
        match recorder {
            Some(r) => metrics::with_local_recorder(r, run),
            None => run(),
        }
    }

    /// What a record costs each way, measured: printed, held only to the kept handles costing
    /// less than upstream's lookups (lbterm/PERF.md has the numbers)
    #[test]
    fn a_kept_handle_costs_less_than_upstream_s_lookup() {
        let upstream = Upstream::default();
        let kept = Kept::default();
        let frames = 20_000;
        // Warm: every key registered with both
        per_record(Some(&upstream), 10);
        per_record(Some(&kept), 10);
        let mut up = vec![];
        let mut lb = vec![];
        let mut none = vec![];
        for _ in 0..3 {
            up.push(per_record(Some(&upstream), frames));
            lb.push(per_record(Some(&kept), frames));
            none.push(per_record(None, frames));
        }
        let med = |v: &mut Vec<f64>| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[v.len() / 2]
        };
        let (u, k, n) = (med(&mut up), med(&mut lb), med(&mut none));
        eprintln!(
            "a record: upstream's lookup {u:.1} ns, a kept handle {k:.1} ns, no recorder {n:.1} ns \
             (each run's: {up:.1?}, {lb:.1?}, {none:.1?})"
        );
        let total: u64 = upstream
            .0
            .lock()
            .unwrap()
            .values()
            .map(|c| c.0.load(Relaxed))
            .sum();
        let total_kept: u64 = kept
            .0
             .0
            .lock()
            .unwrap()
            .values()
            .map(|c| c.0.load(Relaxed))
            .sum();
        assert_eq!(total, total_kept, "both recorded every one");
        assert!(
            k < u,
            "a kept handle {k:.1} ns, upstream's lookup {u:.1} ns"
        );
    }
}
