//! How long each step of making a window takes (lbterm/PERF.md, "What font setup costs"): its
//! fonts, the operating system's window, its GPU state step by step (the instance, the adapter,
//! the device, the shader and the pipeline), its render state. Each run of [`Steps`] is said on
//! `lbterm::start` at debug when it's done, and the last of each kind is kept for the control
//! pipe's `metrics` (`start`), so a test can say where a window's start went without a log. The
//! process's own start is marked as it goes ([`mark`]), each mark and each kind's start in ms
//! since `main` began ([`began`]), so what waits on what shows.

use serde_json::{json, Map, Value};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static BEGAN: OnceLock<Instant> = OnceLock::new();
static MARKS: Mutex<Vec<(&'static str, f64)>> = Mutex::new(Vec::new());
/// The compositor's surfaces made (a visual's swapchain each), and their time in ns
static SURFACES: (AtomicU64, AtomicU64) = (AtomicU64::new(0), AtomicU64::new(0));

/// The compositor made a surface, taking `took`: a window's first frame makes one for each part
/// of it, which is most of that frame's draw
pub fn surface_made(took: Duration) {
    SURFACES.0.fetch_add(1, Relaxed);
    SURFACES.1.fetch_add(took.as_nanos() as u64, Relaxed);
}

/// `main` began: what the marks and the steps' starts are timed from
pub fn began(at: Instant) {
    let _ = BEGAN.set(at);
}

fn since_began(at: Instant) -> Option<f64> {
    BEGAN.get().map(|b| ms(at.saturating_duration_since(*b)))
}

/// The process's start got as far as `name`, now (the first time only)
pub fn mark(name: &'static str) {
    let Some(at) = since_began(Instant::now()) else {
        return;
    };
    let mut marks = MARKS.lock().unwrap_or_else(|e| e.into_inner());
    if !marks.iter().any(|(n, _)| *n == name) {
        marks.push((name, at));
    }
}

/// The steps of one thing being made, timed one after another
pub struct Steps {
    what: &'static str,
    began: Instant,
    at: Instant,
    steps: Vec<(&'static str, Duration)>,
}

/// The last run of each kind: (what, its steps in ms, in all ms, when it began in ms since `main`)
static LAST: Mutex<Vec<(&'static str, Vec<(&'static str, f64)>, f64, Option<f64>)>> =
    Mutex::new(Vec::new());

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1_000_000.0).round() / 1000.0
}

impl Steps {
    pub fn new(what: &'static str) -> Self {
        let now = Instant::now();
        Self {
            what,
            began: now,
            at: now,
            steps: vec![],
        }
    }

    /// The step `name` ended now: the time since the last step's end
    pub fn step(&mut self, name: &'static str) {
        let now = Instant::now();
        self.steps.push((name, now - self.at));
        self.at = now;
    }

    /// The steps so far and the whole, in ms, without saying or keeping them
    pub fn read(&self) -> (Vec<(&'static str, f64)>, f64) {
        (
            self.steps.iter().map(|(n, d)| (*n, ms(*d))).collect(),
            ms(self.at - self.began),
        )
    }

    /// Said on `lbterm::start` at debug, and kept as the last of its kind
    pub fn done(self) {
        let (steps, total) = self.read();
        let at = since_began(self.began);
        log::debug!(
            target: "lbterm::start",
            "{} {:.1} ms{}: {}",
            self.what,
            total,
            at.map(|at| format!(", from {at:.1} ms")).unwrap_or_default(),
            steps
                .iter()
                .map(|(n, ms)| format!("{n} {ms:.1}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        last.retain(|(w, _, _, _)| *w != self.what);
        last.push((self.what, steps, total, at));
    }
}

/// `metrics`' `start`: the last of each kind, `{what: {ms, at_ms, steps: {name: ms}}}`, the
/// process's marks, `marks: {name: at_ms}`, and the compositor's surfaces made since it began,
/// `surfaces: {count, ms}`
pub fn report_json() -> Value {
    let last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let mut o = Map::new();
    let mut marks = Map::new();
    for (name, at) in MARKS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        marks.insert((*name).into(), json!(at));
    }
    o.insert("marks".into(), Value::Object(marks));
    o.insert(
        "surfaces".into(),
        json!({
            "count": SURFACES.0.load(Relaxed),
            "ms": ms(Duration::from_nanos(SURFACES.1.load(Relaxed))),
        }),
    );
    for (what, steps, total, at) in last.iter() {
        let mut s = Map::new();
        for (name, ms) in steps {
            // A step that comes twice (a retry) is said added up
            let was = s.get(*name).and_then(|v| v.as_f64()).unwrap_or(0.0);
            s.insert((*name).into(), json!(was + ms));
        }
        o.insert((*what).into(), json!({ "ms": total, "at_ms": at, "steps": s }));
    }
    Value::Object(o)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn steps_are_timed_one_after_another_and_kept_by_kind() {
        let mut s = Steps::new("test-window");
        std::thread::sleep(Duration::from_millis(2));
        s.step("one");
        s.step("two");
        let (steps, total) = s.read();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].0, "one");
        assert!(steps[0].1 >= 1.5, "the sleep is the first step's: {steps:?}");
        assert!(steps[1].1 < steps[0].1);
        assert!(total >= steps[0].1 + steps[1].1 - 0.01);
        s.done();
        let mut again = Steps::new("test-window");
        again.step("three");
        again.done();
        let report = report_json();
        let w = &report["test-window"];
        assert!(w["steps"]["three"].is_number(), "the last of its kind: {report}");
        assert!(w["steps"]["one"].is_null(), "not the one before");
    }

    #[test]
    fn marks_are_timed_from_main_once_each() {
        began(Instant::now() - Duration::from_millis(50));
        mark("test-mark");
        mark("test-mark");
        let report = report_json();
        let at = report["marks"]["test-mark"].as_f64().unwrap();
        assert!(at >= 49.0, "{report}");
        assert_eq!(
            MARKS.lock().unwrap().iter().filter(|(n, _)| *n == "test-mark").count(),
            1,
            "the first time only"
        );
    }
}
