//! LBTerm: a histogram of durations in fixed atomic buckets (lbterm/PERF.md, "What the budget
//! costs"), for what the GUI times on its own thread as it runs: each action the budget times
//! (budget.rs). Recording one takes four relaxed atomic adds and nothing else: no lock, no
//! allocation, nothing registered, where upstream's `metrics::histogram!` builds its key, takes
//! the recorder's lock to find the histogram and then the histogram's own lock on every call.
//! The pipe's thread reads it for `metrics`, and a reset zeroes each count as it's read.
//!
//! Buckets by the microsecond: exact below 64 µs, then 32 to each doubling (about 3% wide) up to
//! 2^34 µs, about four and a half hours; what's longer goes in the last. A percentile is said as
//! the middle of the bucket it falls in, never past the longest recorded.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Duration;

/// Exact buckets, one a microsecond
const LINEAR: usize = 64;
/// Each doubling past them in 2^SUB_BITS buckets
const SUB_BITS: u32 = 5;
const SUB: usize = 1 << SUB_BITS;
/// Doublings from 64 µs
const OCTAVES: usize = 28;
pub const BUCKETS: usize = LINEAR + OCTAVES * SUB;

/// The bucket `us` microseconds falls in
fn index(us: u64) -> usize {
    if us < LINEAR as u64 {
        return us as usize;
    }
    let e = 63 - us.leading_zeros();
    let sub = ((us >> (e - SUB_BITS)) as usize) & (SUB - 1);
    (LINEAR + (e as usize - 6) * SUB + sub).min(BUCKETS - 1)
}

/// The first microsecond of bucket `i`, and how many it holds
fn bounds(i: usize) -> (u64, u64) {
    if i < LINEAR {
        return (i as u64, 1);
    }
    let e = ((i - LINEAR) / SUB + 6) as u32;
    let s = ((i - LINEAR) % SUB) as u64;
    ((1u64 << e) + (s << (e - SUB_BITS)), 1u64 << (e - SUB_BITS))
}

/// Durations, counted where they fall, with how many, their sum and the longest
pub struct Histogram {
    buckets: [AtomicU64; BUCKETS],
    count: AtomicU64,
    sum_ns: AtomicU64,
    max_ns: AtomicU64,
}

/// What a histogram holds, read: milliseconds
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Read {
    pub count: u64,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

impl Histogram {
    pub const fn new() -> Self {
        Self {
            buckets: [const { AtomicU64::new(0) }; BUCKETS],
            count: AtomicU64::new(0),
            sum_ns: AtomicU64::new(0),
            max_ns: AtomicU64::new(0),
        }
    }

    /// One more
    #[inline]
    pub fn record(&self, d: Duration) {
        let ns = d.as_nanos().min(u64::MAX as u128) as u64;
        self.buckets[index(ns / 1000)].fetch_add(1, Relaxed);
        self.count.fetch_add(1, Relaxed);
        self.sum_ns.fetch_add(ns, Relaxed);
        self.max_ns.fetch_max(ns, Relaxed);
    }

    /// What it holds; with `reset`, each count zeroed as it's read
    pub fn read(&self, reset: bool) -> Read {
        let take = |a: &AtomicU64| if reset { a.swap(0, Relaxed) } else { a.load(Relaxed) };
        let mut counts = [0u64; BUCKETS];
        let mut n = 0u64;
        for (c, b) in counts.iter_mut().zip(self.buckets.iter()) {
            *c = take(b);
            n += *c;
        }
        let count = take(&self.count);
        let sum = take(&self.sum_ns);
        let max = take(&self.max_ns);
        let max_ms = max as f64 / 1e6;
        let at = |q: f64| -> f64 {
            if n == 0 {
                return 0.0;
            }
            let rank = ((q * n as f64).ceil() as u64).clamp(1, n);
            let mut seen = 0u64;
            for (i, c) in counts.iter().enumerate() {
                seen += c;
                if seen >= rank {
                    let (low, width) = bounds(i);
                    let mid_ms = (low as f64 + width as f64 / 2.0) / 1000.0;
                    return mid_ms.min(max_ms);
                }
            }
            max_ms
        };
        Read {
            count,
            mean_ms: if count > 0 { sum as f64 / count as f64 / 1e6 } else { 0.0 },
            p50_ms: at(0.50),
            p95_ms: at(0.95),
            p99_ms: at(0.99),
            max_ms,
        }
    }
}

impl Default for Histogram {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_microsecond_has_a_bucket_and_the_buckets_follow_on() {
        let mut last = None;
        for us in 0..5_000u64 {
            let i = index(us);
            if let Some(l) = last {
                assert!(i == l || i == l + 1, "{us} µs skips from bucket {l} to {i}");
            }
            last = Some(i);
        }
        for us in (0..5_000u64).chain([1 << 20, (1 << 34) - 1]) {
            let i = index(us);
            let (low, width) = bounds(i);
            assert!(low <= us && us < low + width, "{us} µs in bucket {i}: {low}..{}", low + width);
        }
        // About 3% wide past the exact ones
        for i in [LINEAR, LINEAR + 100, BUCKETS - 1] {
            let (low, width) = bounds(i);
            assert!(width as f64 / low as f64 <= 1.0 / SUB as f64 + 1e-9, "bucket {i}: {low} +{width}");
        }
        assert_eq!(index(u64::MAX / 1000), BUCKETS - 1, "past the last: in it");
    }

    #[test]
    fn percentiles_count_and_reset() {
        let h = Histogram::new();
        // 1 ms to 100 ms, one each
        for ms in 1..=100u64 {
            h.record(Duration::from_millis(ms));
        }
        let r = h.read(false);
        assert_eq!(r.count, 100);
        assert!((r.mean_ms - 50.5).abs() < 1e-9, "{r:?}");
        assert!((r.p50_ms - 50.0).abs() / 50.0 < 0.03, "{r:?}");
        assert!((r.p95_ms - 95.0).abs() / 95.0 < 0.03, "{r:?}");
        assert!((r.p99_ms - 99.0).abs() / 99.0 < 0.03, "{r:?}");
        assert_eq!(r.max_ms, 100.0);
        // A reset reads, then zeroes
        assert_eq!(h.read(true), r);
        assert_eq!(h.read(false), Read::default());
        // One alone: every percentile is it, not past it
        h.record(Duration::from_micros(1234));
        let one = h.read(false);
        assert_eq!((one.count, one.max_ms), (1, 1.234));
        assert!(one.p50_ms <= 1.234 && one.p50_ms > 1.2, "{one:?}");
    }

    /// What a record costs, measured: printed, held only to a loose ceiling (lbterm/PERF.md has
    /// the numbers)
    #[test]
    fn a_record_costs_a_few_atomic_adds() {
        static H: Histogram = Histogram::new();
        let n = 1_000_000u32;
        let t = std::time::Instant::now();
        for i in 0..n {
            H.record(Duration::from_nanos(1_000_000 + (i as u64 % 50_000) * 997));
        }
        let per = t.elapsed().as_nanos() as f64 / n as f64;
        let read = std::time::Instant::now();
        let r = H.read(true);
        let read_us = read.elapsed().as_secs_f64() * 1e6;
        eprintln!("a record {per:.1} ns; a read and reset of {} {read_us:.1} µs", r.count);
        assert_eq!(r.count, n as u64);
        assert!(per < 1_000.0, "{per}");
    }
}
