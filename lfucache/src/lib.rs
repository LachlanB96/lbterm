use ahash::AHasher;
use config::ConfigHandle;
use intrusive_collections::{
    intrusive_adapter, KeyAdapter, LinkedList, LinkedListLink, RBTree, RBTreeLink,
};
use std::borrow::Borrow;
use std::cell::RefCell;
use std::cmp::Eq;
use std::fmt::Debug;
use std::hash::{BuildHasher, BuildHasherDefault, Hash, Hasher};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;

/// LBTerm: what the caches of one name did (lbterm/PERF.md, "What the caches keep"): lookups
/// that found their entry, those that didn't, and entries put out to make room, counted with a
/// relaxed atomic add each, where every lookup took upstream's `metrics::histogram!` (the
/// recorder's global lock, a SipHash lookup, the histogram's own lock and the clock). Every cache
/// of a name counts into one, as the histograms of a name did; the GUI's `metrics` reads them.
pub struct Counts {
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: AtomicU64,
}

/// One name's counts, read
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Read {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

impl Counts {
    const fn new() -> Self {
        Self {
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
        }
    }

    #[inline]
    pub fn hit(&self) {
        self.hits.fetch_add(1, Relaxed);
    }

    #[inline]
    pub fn miss(&self) {
        self.misses.fetch_add(1, Relaxed);
    }

    #[inline]
    fn evicted(&self) {
        self.evictions.fetch_add(1, Relaxed);
    }

    /// What they hold; with `reset`, each zeroed as it's read
    pub fn read(&self, reset: bool) -> Read {
        let take = |a: &AtomicU64| {
            if reset {
                a.swap(0, Relaxed)
            } else {
                a.load(Relaxed)
            }
        };
        Read {
            hits: take(&self.hits),
            misses: take(&self.misses),
            evictions: take(&self.evictions),
        }
    }
}

/// Every name's counts, made once each and kept for the process
static NAMED: Mutex<Vec<(&'static str, &'static Counts)>> = Mutex::new(Vec::new());

/// The counts of the caches named `name` (a name upstream gave its hit histogram, its
/// `.hit.rate` left off): made the first time, the same ones after
pub fn counts(name: &'static str) -> &'static Counts {
    let name = name.strip_suffix(".hit.rate").unwrap_or(name);
    let mut named = NAMED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, c)) = named.iter().find(|(n, _)| *n == name) {
        return c;
    }
    let c: &'static Counts = Box::leak(Box::new(Counts::new()));
    named.push((name, c));
    c
}

/// Every name's counts, by name; with `reset`, each zeroed as it's read
pub fn report(reset: bool) -> Vec<(&'static str, Read)> {
    let named = NAMED.lock().unwrap_or_else(|e| e.into_inner());
    let mut all: Vec<_> = named.iter().map(|(n, c)| (*n, c.read(reset))).collect();
    all.sort_by_key(|(n, _)| *n);
    all
}

struct Entry<K, V> {
    hash_link: LinkedListLink,
    recency_link: LinkedListLink,
    frequency_link: RBTreeLink,
    freq: RefCell<u16>,
    last_tick: RefCell<u64>,
    key: K,
    value: V,
}

intrusive_adapter!(RecencyAdapter<K,V> = Rc<Entry<K,V>>: Entry<K,V> { recency_link: LinkedListLink });
intrusive_adapter!(FrequenceAdapter<K,V> = Rc<Entry<K,V>>: Entry<K,V> { frequency_link: RBTreeLink });
intrusive_adapter!(HashAdapter<K,V> = Rc<Entry<K,V>>: Entry<K,V> { hash_link: LinkedListLink });

/// Key by Entry::freq, then by its last use: of the least frequently used, the least recently
/// used comes first. LBTerm: upstream's went by the count alone, which put an entry whose count
/// had just decayed to nothing after the entries just put in, so they went before it (finding
/// fonts-8)
impl<'a, K, V> KeyAdapter<'a> for FrequenceAdapter<K, V> {
    type Key = (u16, u64);
    fn get_key(&self, entry: &'a Entry<K, V>) -> (u16, u64) {
        (*entry.freq.borrow(), *entry.last_tick.borrow())
    }
}

pub type CapFunc = fn(&ConfigHandle) -> usize;

/// A cache using a Least-Frequently-Used eviction policy.
/// If K is u64 you should use LfuCacheU64 instead as it has
/// a more optimal hasher for integer keys.
pub struct LfuCache<K, V, S = BuildHasherDefault<AHasher>> {
    /// LBTerm: its name's counts, where upstream recorded its hit and miss histograms
    counts: &'static Counts,
    cap: usize,
    cap_func: CapFunc,
    hasher: S,

    /// hash buckets for key-based lookup
    buckets: Vec<LinkedList<HashAdapter<K, V>>>,
    /// frequency-keyed rb-tree
    frequency_index: RBTree<FrequenceAdapter<K, V>>,
    /// the back is the least-recently-used whereas the front is the
    /// most-recently-used
    recency_index: LinkedList<RecencyAdapter<K, V>>,
    /// Number of items in the cache
    len: usize,
    /// tracks number of operations that affect the frequency/age of entries
    /// (LBTerm: 64 bits, where 32 went round after a few days of a busy window)
    tick: u64,
}

impl<K: Hash + Eq + Clone + Debug, V, S: Default + BuildHasher> LfuCache<K, V, S> {
    #[cfg(test)]
    fn with_capacity(cap: usize) -> Self {
        let mut buckets = vec![];
        let num_buckets = (cap / 10).next_power_of_two();
        for _ in 0..num_buckets {
            buckets.push(LinkedList::new(HashAdapter::new()));
        }

        let hasher = S::default();

        fn dummy_cap_func(_: &ConfigHandle) -> usize {
            8
        }

        Self {
            counts: counts("test"),
            cap,
            cap_func: dummy_cap_func,
            buckets,
            frequency_index: RBTree::new(FrequenceAdapter::new()),
            recency_index: LinkedList::new(RecencyAdapter::new()),
            len: 0,
            tick: 0,
            hasher,
        }
    }

    pub fn new(
        hit: &'static str,
        _miss: &'static str,
        cap_func: CapFunc,
        config: &ConfigHandle,
    ) -> Self {
        let cap = cap_func(config);
        let mut buckets = vec![];
        let num_buckets = (cap / 10).next_power_of_two();
        for _ in 0..num_buckets {
            buckets.push(LinkedList::new(HashAdapter::new()));
        }

        let hasher = S::default();

        Self {
            counts: counts(hit),
            cap,
            cap_func,
            buckets,
            frequency_index: RBTree::new(FrequenceAdapter::new()),
            recency_index: LinkedList::new(RecencyAdapter::new()),
            len: 0,
            tick: 0,
            hasher,
        }
    }

    fn bucket_for_key<Q: Hash>(&self, k: &Q) -> usize {
        let mut hasher = self.hasher.build_hasher();
        k.hash(&mut hasher);
        (hasher.finish() as usize) % self.buckets.len()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// Grow the hash buckets in the pursuit of reducing potential
    /// key collisions in any given bucket
    fn grow_hash(&mut self) {
        let num_buckets = self.buckets.len() * 2;
        let mut buckets = vec![];
        for _ in 0..num_buckets {
            buckets.push(LinkedList::new(HashAdapter::new()));
        }
        std::mem::swap(&mut buckets, &mut self.buckets);

        for mut old_bucket in buckets {
            while let Some(entry) = old_bucket.pop_front() {
                let bucket = self.bucket_for_key(&entry.key);
                self.buckets[bucket].push_front(entry);
            }
        }
    }

    pub fn update_config(&mut self, config: &ConfigHandle) {
        let new_cap = (self.cap_func)(config);
        if new_cap != self.cap {
            self.cap = new_cap;
            while self.len > self.cap {
                self.evict_one();
            }
        }
    }

    /// In order to mitigate previously-very-hot entries that are
    /// not currently being accessed from occupying the bulk of the
    /// table, this function finds the least-recently-accessed item
    /// and decreases its frequency value based on the number of ticks
    /// that have occurred since its last use.
    fn decay_least_recent(&mut self) {
        let mut cursor = self.recency_index.back_mut();
        if let Some(entry) = cursor.get() {
            if *entry.freq.borrow() == 0 {
                // No point removing/reinserting in the rbtree if the freq
                // is already 0
                return;
            }

            // LBTerm: at most u16::MAX, where the cast went round past 655,360 ticks
            let delta = ((self.tick - *entry.last_tick.borrow()) / 10).min(u16::MAX as u64) as u16;
            if delta <= 1 {
                // No point removing/reinserting in the rbtree if there is no change
                return;
            }

            // Adjust lfu
            unsafe {
                let lfu_entry = self
                    .frequency_index
                    .cursor_mut_from_ptr(entry)
                    .remove()
                    .unwrap();
                {
                    let mut freq = entry.freq.borrow_mut();
                    *freq = *freq / delta;
                }
                self.frequency_index.insert(lfu_entry);
            }

            // Adjust lru so that we don't immediately revisit this one
            // on the next decay_least_recent() call
            let lru_entry = cursor.remove().unwrap();
            self.recency_index.push_front(lru_entry);
        }
    }

    /// LBTerm: an entry used within this many of the latest ticks is kept whatever its count:
    /// half the cache. A window's frame looks each line in view up once, so that's a few frames'
    /// lines
    fn window(&self) -> u64 {
        (self.cap / 2).max(1) as u64
    }

    /// Whether `entry` was used within the latest `window()` ticks
    fn is_recent(&self, entry: &Entry<K, V>) -> bool {
        self.tick - *entry.last_tick.borrow() < self.window()
    }

    /// Remove the entry with the smallest frequency value (LBTerm: the least recently used of
    /// those)
    ///
    /// LBTerm: unless it was used within the latest `window()` ticks, when the least recently
    /// used goes instead, as long as that one wasn't (finding fonts-8). Every entry is put in
    /// with a count of nothing, so in a cache whose entries had all been looked up many times
    /// lately, the entry put in last was the least frequently used and the next one put in put
    /// it out: lines a frame put in were laid out again in the next, frame after frame, until the
    /// old entries' counts decayed
    fn evict_one(&mut self) {
        self.decay_least_recent();

        let lfu = self.frequency_index.front().get();
        let victim: Option<*const Entry<K, V>> = match lfu {
            Some(lfu) if self.is_recent(lfu) => match self.recency_index.back().get() {
                Some(lru) if !self.is_recent(lru) => Some(lru as *const _),
                _ => Some(lfu as *const _),
            },
            Some(lfu) => Some(lfu as *const _),
            None => None,
        };
        if let Some(victim) = victim {
            unsafe {
                let entry = self
                    .frequency_index
                    .cursor_mut_from_ptr(victim)
                    .remove()
                    .expect("in the frequency index");
                let bucket = self.bucket_for_key(&entry.key);
                self.buckets
                    .get_mut(bucket)
                    .unwrap()
                    .cursor_mut_from_ptr(&*entry)
                    .remove();
                self.recency_index.cursor_mut_from_ptr(&*entry).remove();
            }
            self.len -= 1;
            self.counts.evicted();
        }
    }

    pub fn clear(&mut self) {
        self.frequency_index.clear();
        self.recency_index.clear();
        for bucket in &mut self.buckets {
            bucket.clear();
        }
        self.len = 0;
    }

    pub fn get<'a, Q: ?Sized + Debug>(&'a mut self, k: &Q) -> Option<&'a V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        let bucket = self.bucket_for_key(&k);
        let mut cursor = self.buckets.get_mut(bucket)?.front_mut();
        while let Some(entry) = cursor.get() {
            if entry.key.borrow() == k {
                // Adjust lru
                unsafe {
                    let lru_entry = self
                        .recency_index
                        .cursor_mut_from_ptr(entry)
                        .remove()
                        .unwrap();
                    self.recency_index.push_front(lru_entry);
                }

                let entry = cursor.into_ref()?;
                self.counts.hit();

                self.tick += 1;

                // Adjust lfu (LBTerm: its last use is part of its key there, so it changes while
                // the entry is out)
                unsafe {
                    let lfu_entry = self
                        .frequency_index
                        .cursor_mut_from_ptr(entry)
                        .remove()
                        .unwrap();
                    {
                        let mut freq = lfu_entry.freq.borrow_mut();
                        *freq = freq.saturating_add(1);
                    }
                    *lfu_entry.last_tick.borrow_mut() = self.tick;
                    self.frequency_index.insert(lfu_entry);
                }

                return Some(&entry.value);
            }

            cursor.move_next();
        }
        self.counts.miss();
        None
    }

    pub fn put(&mut self, k: K, v: V) {
        let bucket = self.bucket_for_key(&k);

        self.tick += 1;

        // Remove any prior value
        {
            let mut cursor = self
                .buckets
                .get_mut(bucket)
                .expect("valid bucket index")
                .front_mut();
            while let Some(entry) = cursor.get() {
                if entry.key == k {
                    unsafe {
                        self.frequency_index.cursor_mut_from_ptr(entry).remove();
                        self.recency_index.cursor_mut_from_ptr(entry).remove();
                    }
                    cursor.remove();
                    self.len -= 1;
                    break;
                }
                cursor.move_next();
            }
        }

        while self.len >= self.cap {
            self.evict_one();
        }

        let entry = Rc::new(Entry {
            key: k,
            value: v,
            freq: RefCell::new(0),
            recency_link: LinkedListLink::new(),
            frequency_link: RBTreeLink::new(),
            hash_link: LinkedListLink::new(),
            last_tick: RefCell::new(self.tick),
        });
        self.buckets[bucket].push_front(Rc::clone(&entry));
        self.frequency_index.insert(Rc::clone(&entry));
        self.recency_index.push_front(entry);
        self.len += 1;
        if self.buckets.len() < self.cap && self.len > self.buckets.len() / 2 {
            self.grow_hash();
        }
    }
}

/// A cache using a Least-Frequently-Used eviction policy,
/// where the cache keys are u64
pub type LfuCacheU64<V> = LfuCache<u64, V, fnv::FnvBuildHasher>;

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Debug)]
    #[allow(dead_code)]
    struct EntryData<'a, K, V> {
        freq: u16,
        last_tick: u64,
        key: &'a K,
        value: &'a V,
    }

    impl<'a, K, V> EntryData<'a, K, V> {
        fn new(item: &'a Entry<K, V>) -> Self {
            Self {
                freq: *item.freq.borrow(),
                last_tick: *item.last_tick.borrow(),
                key: &item.key,
                value: &item.value,
            }
        }
    }

    fn frequency_order<K, V, S>(cache: &LfuCache<K, V, S>) -> Vec<EntryData<'_, K, V>> {
        let mut entries = vec![];
        for item in cache.frequency_index.iter() {
            entries.push(EntryData::new(item));
        }
        entries
    }

    fn recency_order<K, V, S>(cache: &LfuCache<K, V, S>) -> Vec<EntryData<'_, K, V>> {
        let mut entries = vec![];
        for item in cache.recency_index.iter() {
            entries.push(EntryData::new(item));
        }
        entries
    }

    #[test]
    fn decay() {
        let mut cache = LfuCacheU64::with_capacity(4);
        for i in 0..4 {
            cache.put(i, i);
            for _ in 0..i * 2 {
                cache.get(&i);
            }
        }
        k9::snapshot!(
            frequency_order(&cache),
            "
[
    EntryData {
        freq: 0,
        last_tick: 1,
        key: 0,
        value: 0,
    },
    EntryData {
        freq: 2,
        last_tick: 4,
        key: 1,
        value: 1,
    },
    EntryData {
        freq: 4,
        last_tick: 9,
        key: 2,
        value: 2,
    },
    EntryData {
        freq: 6,
        last_tick: 16,
        key: 3,
        value: 3,
    },
]
"
        );

        cache.get(&1);
        cache.get(&2);
        cache.put(10, 10);

        k9::snapshot!(
            frequency_order(&cache),
            "
[
    EntryData {
        freq: 0,
        last_tick: 19,
        key: 10,
        value: 10,
    },
    EntryData {
        freq: 3,
        last_tick: 17,
        key: 1,
        value: 1,
    },
    EntryData {
        freq: 5,
        last_tick: 18,
        key: 2,
        value: 2,
    },
    EntryData {
        freq: 6,
        last_tick: 16,
        key: 3,
        value: 3,
    },
]
"
        );

        cache.get(&10);
        // LBTerm: 3 goes to make room, the least recently used, where it was 10, the least
        // frequently used, which was used the tick before (evict_one's window)
        cache.put(11, 11);
        // bump up freq of 11 so that we can displace 10 on the next put
        cache.get(&11);
        cache.get(&11);
        cache.get(&11);
        cache.get(&11);
        k9::snapshot!(
            frequency_order(&cache),
            "
[
    EntryData {
        freq: 1,
        last_tick: 20,
        key: 10,
        value: 10,
    },
    EntryData {
        freq: 3,
        last_tick: 17,
        key: 1,
        value: 1,
    },
    EntryData {
        freq: 4,
        last_tick: 25,
        key: 11,
        value: 11,
    },
    EntryData {
        freq: 5,
        last_tick: 18,
        key: 2,
        value: 2,
    },
]
"
        );

        cache.put(12, 12);
        k9::snapshot!(
            frequency_order(&cache),
            "
[
    EntryData {
        freq: 0,
        last_tick: 26,
        key: 12,
        value: 12,
    },
    EntryData {
        freq: 3,
        last_tick: 17,
        key: 1,
        value: 1,
    },
    EntryData {
        freq: 4,
        last_tick: 25,
        key: 11,
        value: 11,
    },
    EntryData {
        freq: 5,
        last_tick: 18,
        key: 2,
        value: 2,
    },
]
"
        );

        // Ensure that we're all non-zero
        for _ in 0..5 {
            cache.get(&2);
            cache.get(&11);
            cache.get(&12);
        }

        // and bump up the ticks so that we trigger decay for 1
        for _ in 0..10 {
            cache.get(&11);
        }

        // Note that key: 1 has freq 3 in this snapshot
        k9::snapshot!(
            frequency_order(&cache),
            "
[
    EntryData {
        freq: 3,
        last_tick: 17,
        key: 1,
        value: 1,
    },
    EntryData {
        freq: 5,
        last_tick: 41,
        key: 12,
        value: 12,
    },
    EntryData {
        freq: 10,
        last_tick: 39,
        key: 2,
        value: 2,
    },
    EntryData {
        freq: 19,
        last_tick: 51,
        key: 11,
        value: 11,
    },
]
"
        );

        // trigger an eviction. This will decay key 1's freq
        // from 3 to 1, unused for 35 ticks, and it will be evicted.
        cache.put(42, 42);
        k9::snapshot!(
            frequency_order(&cache),
            "
[
    EntryData {
        freq: 0,
        last_tick: 52,
        key: 42,
        value: 42,
    },
    EntryData {
        freq: 5,
        last_tick: 41,
        key: 12,
        value: 12,
    },
    EntryData {
        freq: 10,
        last_tick: 39,
        key: 2,
        value: 2,
    },
    EntryData {
        freq: 19,
        last_tick: 51,
        key: 11,
        value: 11,
    },
]
"
        );
    }

    #[test]
    fn eviction() {
        let mut cache = LfuCacheU64::with_capacity(8);
        for i in 0..8 {
            cache.put(i, i);
            for _ in 0..i {
                cache.get(&i);
            }
        }

        k9::assert_equal!(cache.len(), 8);
        cache.put(8, 8);
        k9::assert_equal!(cache.len(), 8);

        let freq = frequency_order(&cache);
        k9::assert_equal!(*freq[0].key, 8, "0 got evicted, so 8 is first");
        k9::snapshot!(
            freq,
            "
[
    EntryData {
        freq: 0,
        last_tick: 37,
        key: 8,
        value: 8,
    },
    EntryData {
        freq: 1,
        last_tick: 3,
        key: 1,
        value: 1,
    },
    EntryData {
        freq: 2,
        last_tick: 6,
        key: 2,
        value: 2,
    },
    EntryData {
        freq: 3,
        last_tick: 10,
        key: 3,
        value: 3,
    },
    EntryData {
        freq: 4,
        last_tick: 15,
        key: 4,
        value: 4,
    },
    EntryData {
        freq: 5,
        last_tick: 21,
        key: 5,
        value: 5,
    },
    EntryData {
        freq: 6,
        last_tick: 28,
        key: 6,
        value: 6,
    },
    EntryData {
        freq: 7,
        last_tick: 36,
        key: 7,
        value: 7,
    },
]
"
        );

        for i in 9..12 {
            cache.put(i, i);
            cache.get(&i);
        }
        k9::snapshot!(
            frequency_order(&cache),
            "
[
    EntryData {
        freq: 1,
        last_tick: 10,
        key: 3,
        value: 3,
    },
    EntryData {
        freq: 1,
        last_tick: 39,
        key: 9,
        value: 9,
    },
    EntryData {
        freq: 1,
        last_tick: 41,
        key: 10,
        value: 10,
    },
    EntryData {
        freq: 1,
        last_tick: 43,
        key: 11,
        value: 11,
    },
    EntryData {
        freq: 4,
        last_tick: 15,
        key: 4,
        value: 4,
    },
    EntryData {
        freq: 5,
        last_tick: 21,
        key: 5,
        value: 5,
    },
    EntryData {
        freq: 6,
        last_tick: 28,
        key: 6,
        value: 6,
    },
    EntryData {
        freq: 7,
        last_tick: 36,
        key: 7,
        value: 7,
    },
]
"
        );
    }

    /// LBTerm (lbterm/PERF.md, "What the caches keep", finding fonts-8): a full cache of keys
    /// looked up 100 times each, then rounds of lookups of 32 keys it doesn't have, each put on
    /// its miss, as a frame does a tab's new lines; the misses in each round
    fn rounds_after_a_warm_cache(interleaved: bool) -> Vec<usize> {
        let mut cache = LfuCacheU64::<u64>::with_capacity(64);
        for k in 0..64 {
            cache.put(k, k);
        }
        if interleaved {
            for _ in 0..100 {
                for k in 0..64 {
                    cache.get(&k);
                }
            }
        } else {
            for k in 0..64 {
                for _ in 0..100 {
                    cache.get(&k);
                }
            }
        }
        (0..20)
            .map(|_| {
                let mut misses = 0;
                for k in 1000..1032 {
                    if cache.get(&k).is_none() {
                        misses += 1;
                        cache.put(k, k);
                    }
                }
                misses
            })
            .collect()
    }

    #[test]
    fn new_keys_stay_after_the_round_that_put_them() {
        let both =
            [true, false].map(|interleaved| (interleaved, rounds_after_a_warm_cache(interleaved)));
        for (interleaved, misses) in &both {
            eprintln!("warmed interleaved {interleaved}: misses a round {misses:?}");
        }
        for (interleaved, misses) in both {
            assert_eq!(misses[0], 32, "interleaved {interleaved}: {misses:?}");
            assert!(
                misses[1..].iter().all(|&m| m == 0),
                "interleaved {interleaved}: the new keys missed again after the round that put them: {misses:?}"
            );
        }
    }

    /// A window's frames: `tabs` tabs of `lines` lines each shown `shown` frames in turn,
    /// `cycles` times round, every line of the shown tab looked up each frame and put on a miss
    /// (the line quad cache's way, upstream's 1024 entries), then a tab not shown before: its
    /// misses in each of its first 30 frames
    fn frames_of_a_new_tab(tabs: u64, lines: u64, shown: usize, cycles: usize) -> Vec<usize> {
        let mut cache = LfuCacheU64::<u64>::with_capacity(1024);
        let frame = |cache: &mut LfuCacheU64<u64>, tab: u64| {
            let mut misses = 0;
            for line in 0..lines {
                let k = tab * 10_000 + line;
                if cache.get(&k).is_none() {
                    misses += 1;
                    cache.put(k, k);
                }
            }
            misses
        };
        for _ in 0..cycles {
            for tab in 0..tabs {
                for _ in 0..shown {
                    frame(&mut cache, tab);
                }
            }
        }
        (0..30).map(|_| frame(&mut cache, tabs)).collect()
    }

    #[test]
    fn a_new_tab_s_lines_stay_in_a_cache_full_of_others() {
        for (tabs, shown, cycles) in [(17, 40, 2), (20, 40, 2), (17, 400, 1), (40, 5, 3)] {
            let misses = frames_of_a_new_tab(tabs, 60, shown, cycles);
            eprintln!(
                "{tabs} tabs of 60 lines shown {shown} frames each, {cycles} times round: \
                 a new tab's misses a frame {misses:?}"
            );
            assert_eq!(misses[0], 60, "{misses:?}");
            assert!(
                misses[1..].iter().all(|&m| m == 0),
                "{tabs} tabs, {shown} frames, {cycles} cycles: the new tab's lines missed again: {misses:?}"
            );
        }
    }

    #[test]
    fn hits_misses_and_evictions_are_counted_by_name() {
        let mut cache = LfuCacheU64::<u64>::with_capacity(4);
        cache.counts = counts("lfucache_test.hit.rate");
        assert!(
            std::ptr::eq(cache.counts, counts("lfucache_test")),
            "the same name's counts"
        );
        for k in 0..6 {
            cache.put(k, k);
        }
        assert!(cache.get(&5).is_some());
        assert!(cache.get(&0).is_none());
        let read = report(false)
            .into_iter()
            .find(|(n, _)| *n == "lfucache_test")
            .expect("named")
            .1;
        assert_eq!(
            read,
            Read {
                hits: 1,
                misses: 1,
                evictions: 2
            }
        );
        assert_eq!(cache.counts.read(true), read);
        assert_eq!(
            cache.counts.read(false),
            Read::default(),
            "zeroed by the reset"
        );
    }

    #[test]
    fn basic() {
        let mut cache = LfuCacheU64::<&'static str>::with_capacity(8);
        cache.put(1, "hello");
        cache.put(2, "there");

        k9::snapshot!(
            frequency_order(&cache),
            r#"
[
    EntryData {
        freq: 0,
        last_tick: 1,
        key: 1,
        value: "hello",
    },
    EntryData {
        freq: 0,
        last_tick: 2,
        key: 2,
        value: "there",
    },
]
"#
        );

        cache.get(&1);
        cache.get(&1);
        cache.get(&1);
        cache.get(&2);

        k9::snapshot!(
            frequency_order(&cache),
            r#"
[
    EntryData {
        freq: 1,
        last_tick: 6,
        key: 2,
        value: "there",
    },
    EntryData {
        freq: 3,
        last_tick: 5,
        key: 1,
        value: "hello",
    },
]
"#
        );

        k9::snapshot!(
            recency_order(&cache),
            r#"
[
    EntryData {
        freq: 1,
        last_tick: 6,
        key: 2,
        value: "there",
    },
    EntryData {
        freq: 3,
        last_tick: 5,
        key: 1,
        value: "hello",
    },
]
"#
        );

        cache.get(&1);
        k9::snapshot!(
            recency_order(&cache),
            r#"
[
    EntryData {
        freq: 4,
        last_tick: 7,
        key: 1,
        value: "hello",
    },
    EntryData {
        freq: 1,
        last_tick: 6,
        key: 2,
        value: "there",
    },
]
"#
        );
    }
}
