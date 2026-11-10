//! Tabs in flight (lbterm/COMPOSITOR.md, "Motion"): the tabs switched from or closed while
//! others still fade out, each over the one after it, and the strip's words and the strips down
//! the window's right with their panes.
//!
//! Each one goes out by its share of the screen: as much of it as showed when it was left, going
//! to nothing on `tab_switch` from then, so no tab in flight ever shows more of itself than it
//! did a frame before. Its visuals lie in a stack, the oldest on top, and a layer shows its
//! opacity of what the layers over it leave, so the opacity that shows a share is that share
//! over what the whole stack above it leaves, `share / (1 - shares above)`
//! ([`Flight::opacity`]): no one curve, so DComp plays it from samples
//! ([`Flight::opacity_segments`]). The tab left goes in at the bottom of the stack from what
//! showed of it (`dissolve_from`, all but what the stack covered), so it starts at its whole
//! opacity. A tab switched back to while it's in flight comes out of the stack, whole, under all
//! of it, and takes its share from the whole stack above it: every one left after it lay under it
//! and loses it from above, so each is played again from now at the opacity that keeps its share,
//! and nothing on screen moves at the switch ([`Flight::switch`]).
//!
//! Until 2026-10-31 each layer's opacity went on `tab_switch` itself, from the share its tab
//! started from: a tab left while an older one still faded over it then showed more of itself as
//! the older one went (in a model of the curve at `slowdown = 5`, five switches 40 ms apart, the
//! second tab from 0.17 to 0.31 after it was left), and a switch back to the oldest of three or
//! more left the newer ones' opacity as it was, though the tab coming back no longer lay over
//! them: the second tab's share jumped from 0.31 to 0.53 and the first's from 0.42 to 0.01, its
//! panes all but gone and coming back.
//!
//! The strip's words are a stack of their own (wezterm-gui's `prepare_swap`): words changing in
//! one tab go in it too, and it's kept to a depth ([`Flight::switch`]'s `depth`).

use crate::curve::Curve;
use crate::motion::{sample, Motion, Segments};
use crate::plan::dissolve_from;

/// What's too little to show: a layer at this share is done
const NOTHING: f64 = 1e-4;

/// How deep the strip's words in flight go: each is a surface the size of the strip, and in a run
/// of quick switches each new one past the two kept spare is made again, which waits for the GPU
/// (4 to 25 ms of the GUI thread). Six holds every one with anything to show at the rates a person
/// switches tabs: at real speed, switches 40 ms apart (side buttons or the rail's pads clicked as
/// fast as a hand goes) leave at most four in flight showing a level of light (1/255) or more and
/// put nothing away, and held keys repeating 33 ms apart five. Past it, only switches faster than
/// that (posted, or the cli) put one away, the one showing least, at once: its share of the strip
/// is the step, 0.26% at 25 ms apart, 2.2% posted every frame at 60 Hz and 9% at 160 Hz
/// (`past_its_depth_nothing_seen_goes_at_a_persons_rates`). The panes themselves have no depth:
/// each tab's are its own surfaces whatever the stack.
pub const WORDS_DEPTH: usize = 6;

/// One in flight: what it is (the panes that left with the tab), the share of the screen it
/// showed as it was left, when (seconds on the caller's clock) and the curve it goes out on.
#[derive(Debug, Clone, PartialEq)]
pub struct Leaving<K> {
    pub key: K,
    pub from: f64,
    pub start: f64,
    pub curve: Curve,
}

impl<K> Leaving<K> {
    fn motion(&self) -> Motion {
        Motion::new(self.from, 0.0, 0.0, self.curve.clone())
    }

    /// Its share of the screen at `t`, and how fast that's changing (per second)
    pub fn share(&self, t: f64) -> (f64, f64) {
        let local = t - self.start;
        if local <= 0.0 {
            return (self.from, 0.0);
        }
        if t >= self.end() {
            return (0.0, 0.0);
        }
        let m = self.motion();
        (m.value(local).max(0.0), m.velocity(local))
    }

    /// When it's gone, on the caller's clock
    pub fn end(&self) -> f64 {
        self.start + self.motion().end(NOTHING)
    }
}

/// What a switch did to the stack.
#[derive(Debug, Clone, PartialEq)]
pub struct Switched<K> {
    /// The share the tab left went in from, at the bottom of the stack; `None` when there was
    /// none, or it showed nothing (it goes at once)
    pub left: Option<f64>,
    /// The one taken out because its tab came back, with the share it showed
    pub back: Option<(K, f64)>,
    /// Put away past the stack's depth, each with the share it showed
    pub dropped: Vec<(K, f64)>,
    /// Every one in flight whose opacity changed, by its place in [`Flight::leaving`], to be
    /// played again from now (`opacity_segments`): the tab left, and all under one taken out
    pub redrive: Vec<usize>,
}

/// The tabs in flight, oldest first, and the oldest on top.
#[derive(Debug, Clone, PartialEq)]
pub struct Flight<K> {
    leaving: Vec<Leaving<K>>,
}

impl<K> Default for Flight<K> {
    fn default() -> Self {
        Self { leaving: vec![] }
    }
}

impl<K: Clone> Flight<K> {
    pub fn leaving(&self) -> &[Leaving<K>] {
        &self.leaving
    }

    pub fn is_empty(&self) -> bool {
        self.leaving.is_empty()
    }

    pub fn len(&self) -> usize {
        self.leaving.len()
    }

    /// How much of the screen the stack shows at `t`: all but what the tab on screen does
    pub fn covered(&self, t: f64) -> f64 {
        self.leaving.iter().map(|l| l.share(t).0).sum::<f64>().clamp(0.0, 1.0)
    }

    /// The opacity at `t` that shows the `i`th its share under the ones above it, and how fast it
    /// changes: its share over what they leave
    pub fn opacity(&self, i: usize, t: f64) -> (f64, f64) {
        let (s, v) = self.leaving[i].share(t);
        let (above, above_v) = self.leaving[..i]
            .iter()
            .map(|l| l.share(t))
            .fold((0.0, 0.0), |(a, av), (s, v)| (a + s, av + v));
        let left = 1.0 - above;
        if left <= 1e-9 {
            return (0.0, 0.0);
        }
        let alpha = s / left;
        if alpha >= 1.0 {
            return (1.0, 0.0);
        }
        (alpha.max(0.0), v / left + s * above_v / (left * left))
    }

    /// The `i`th's opacity from `t` until it's gone, as the cubics DComp plays (`step` and
    /// `epsilon` as for `Motion::segments`)
    pub fn opacity_segments(&self, i: usize, t: f64, step: f64, epsilon: f64) -> Segments {
        let end = self.leaving[i].end() - t;
        sample(|tau| self.opacity(i, t + tau), end, 0.0, step, epsilon)
    }

    /// What the tab on screen shows from `t` until the stack's gone (all but what it covers),
    /// as the cubics DComp plays: how a tab come back shows more of itself, in its own time
    pub fn uncovered_segments(&self, t: f64, step: f64, epsilon: f64) -> Segments {
        let end = self.leaving.iter().map(|l| l.end()).fold(t, f64::max) - t;
        sample(
            |tau| {
                let (s, v) = self
                    .leaving
                    .iter()
                    .map(|l| l.share(t + tau))
                    .fold((0.0, 0.0), |(a, av), (s, v)| (a + s, av + v));
                ((1.0 - s).clamp(0.0, 1.0), -v)
            },
            end,
            1.0,
            step,
            epsilon,
        )
    }

    /// The tab on screen gives way at `t`: the one in flight that `back` picks (its tab is on
    /// screen again) comes out of the stack, and the tab `left`, with what showed of it, goes in
    /// at the bottom on `curve`. Past `depth` the one showing least goes at once. Every one whose
    /// opacity has to change to keep its share is in `redrive`.
    pub fn switch(
        &mut self,
        left: Option<K>,
        back: impl Fn(&K) -> bool,
        t: f64,
        curve: &Curve,
        depth: usize,
    ) -> Switched<K> {
        // What the tab on screen showed, before anything comes out
        let from = dissolve_from(1.0, self.covered(t));
        let mut lowest = usize::MAX;
        let back = self.leaving.iter().position(|l| back(&l.key)).map(|i| {
            let share = self.leaving[i].share(t).0;
            lowest = i;
            (self.leaving.remove(i).key, share)
        });
        let left = left.and_then(|key| {
            (from > NOTHING).then(|| {
                lowest = lowest.min(self.leaving.len());
                self.leaving.push(Leaving {
                    key,
                    from,
                    start: t,
                    curve: curve.clone(),
                });
                from
            })
        });
        let mut dropped = vec![];
        while self.leaving.len() > depth.max(1) {
            // The newest just went in: of the rest, the one showing least
            let newest = self.leaving.len() - 1;
            let least = (0..newest)
                .min_by(|&a, &b| self.leaving[a].share(t).0.total_cmp(&self.leaving[b].share(t).0))
                .unwrap_or(0);
            lowest = lowest.min(least);
            let share = self.leaving[least].share(t).0;
            dropped.push((self.leaving.remove(least).key, share));
        }
        let redrive = (lowest.min(self.leaving.len())..self.leaving.len()).collect();
        Switched {
            left,
            back,
            dropped,
            redrive,
        }
    }

    /// Those gone by `t` come out: nothing shows of them, so nothing else changes
    pub fn put_away_done(&mut self, t: f64) -> Vec<K> {
        let mut done = vec![];
        self.leaving.retain(|l| {
            let gone = l.end() <= t;
            if gone {
                done.push(l.key.clone());
            }
            !gone
        });
        done
    }

    /// All of them out at once (the window resized, motion off)
    pub fn clear(&mut self) -> Vec<K> {
        self.leaving.drain(..).map(|l| l.key).collect()
    }

    /// The place in the stack of the one `pick` takes
    pub fn position(&self, pick: impl Fn(&K) -> bool) -> Option<usize> {
        self.leaving.iter().position(|l| pick(&l.key))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const STEP: f64 = 1.0 / 120.0;
    const EPSILON: f64 = 0.002;

    fn curve(slowdown: f64) -> Curve {
        "200ms cubic-bezier(0.23, 1, 0.32, 1)"
            .parse::<Curve>()
            .unwrap()
            .slowed(slowdown)
    }

    /// What each one in flight and the tab on screen (`None`) show at `t`, by drawing the stack
    /// as DWM does: each layer its opacity of what the layers over it leave
    fn composed(f: &Flight<u32>, t: f64) -> Vec<(Option<u32>, f64)> {
        let mut out = vec![];
        let mut left = 1.0;
        for (i, l) in f.leaving().iter().enumerate() {
            let a = f.opacity(i, t).0;
            out.push((Some(l.key), left * a));
            left *= 1.0 - a;
        }
        out.push((None, left));
        out
    }

    fn shown(f: &Flight<u32>, t: f64, tab: Option<u32>) -> f64 {
        composed(f, t).iter().filter(|(k, _)| *k == tab).map(|(_, s)| s).sum()
    }

    /// What each tab shows at `t`, the one `on` screen included
    fn tab_shares(f: &Flight<u32>, t: f64, tabs: u32, on: u32) -> Vec<f64> {
        (1..=tabs)
            .map(|tab| if tab == on { shown(f, t, None) } else { shown(f, t, Some(tab)) })
            .collect()
    }

    /// Tabs 1 to `tabs` switched through `gap` seconds apart and back to the first, as Lachlan's
    /// side buttons or clicks on the rail's pads would: what each tab shows, every millisecond
    /// for two seconds, with the tab on screen, and the most any tab's share moved at a switch
    fn run(tabs: u32, gap: f64, slowdown: f64) -> (Vec<(f64, u32, Vec<f64>)>, f64) {
        let curve = curve(slowdown);
        let mut f = Flight::default();
        let mut on = 1;
        let mut switches: Vec<(f64, u32)> = (2..=tabs).map(|tab| (gap * (tab - 2) as f64, tab)).collect();
        switches.push((gap * (tabs - 1) as f64, 1));
        let mut next = 0;
        let mut out = vec![];
        let mut step = 0.0_f64;
        for ms in 0..2000 {
            let t = ms as f64 / 1000.0;
            while next < switches.len() && switches[next].0 <= t + 1e-12 {
                let to = switches[next].1;
                let before = tab_shares(&f, t, tabs, on);
                f.switch(Some(on), |k| *k == to, t, &curve, usize::MAX);
                on = to;
                let after = tab_shares(&f, t, tabs, on);
                step = before.iter().zip(&after).map(|(a, b)| (a - b).abs()).fold(step, f64::max);
                next += 1;
            }
            out.push((t, on, tab_shares(&f, t, tabs, on)));
        }
        (out, step)
    }

    #[test]
    fn what_shows_is_each_ones_share() {
        // Drawn as DWM draws the stack, every one in flight shows its own share, and the tab on
        // screen the rest, at every moment of a fast run and its switch back
        let curve = curve(5.0);
        let mut f = Flight::default();
        let mut on = 1;
        for (k, to) in [2, 3, 4, 5, 1].into_iter().enumerate() {
            let t = 0.04 * k as f64;
            f.switch(Some(on), |key| *key == to, t, &curve, usize::MAX);
            on = to;
            for ms in 0..40 {
                let t = t + ms as f64 / 1000.0;
                for (i, l) in f.leaving().iter().enumerate() {
                    let drawn = shown(&f, t, Some(l.key));
                    assert!((drawn - l.share(t).0).abs() < 1e-9, "{} at {t}: {drawn} for {}", l.key, l.share(t).0);
                    assert!((0.0..=1.0).contains(&f.opacity(i, t).0));
                }
                assert!((shown(&f, t, None) - (1.0 - f.covered(t))).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn a_tab_left_starts_whole_and_goes_on_tab_switch() {
        let curve = curve(1.0);
        let mut f = Flight::default();
        let s = f.switch(Some(1), |_| false, 0.0, &curve, usize::MAX);
        assert_eq!(s, Switched { left: Some(1.0), back: None, dropped: vec![], redrive: vec![0] });
        // Alone in flight its opacity is its share, the curve itself
        let m = Motion::new(1.0, 0.0, 0.0, curve.clone());
        for ms in [0, 10, 40, 100, 199] {
            let t = ms as f64 / 1000.0;
            assert!((f.opacity(0, t).0 - m.value(t)).abs() < 1e-12, "at {t}");
        }
        assert_eq!(f.opacity(0, 0.25).0, 0.0);
        assert_eq!(f.put_away_done(0.25), vec![1]);
        assert!(f.is_empty());
    }

    #[test]
    fn a_switch_back_starts_from_what_shows() {
        // One to two, then back to one with one at `a`: two starts from all but `a`, as
        // `dissolve_from` had it, and one shows `a` and rises
        let curve = curve(1.0);
        for back_at in [0.01, 0.03, 0.08] {
            let mut f = Flight::default();
            f.switch(Some(1), |_| false, 0.0, &curve, usize::MAX);
            let a = f.leaving()[0].share(back_at).0;
            let s = f.switch(Some(2), |k| *k == 1, back_at, &curve, usize::MAX);
            assert_eq!(s.back, Some((1, a)));
            assert_eq!(s.left, Some(dissolve_from(1.0, a)));
            assert_eq!(s.redrive, vec![0]);
            assert!((f.opacity(0, back_at).0 - (1.0 - a)).abs() < 1e-12);
            assert!((shown(&f, back_at, None) - a).abs() < 1e-12);
        }
    }

    #[test]
    fn no_tab_in_flight_ever_shows_more_or_jumps() {
        // Five tabs switched through 40 ms apart and back to the first, at slowdown 5 (as
        // perf\tab-switch.ps1 does it) and at real speed, and faster and slower: no tab's share
        // moves at a switch, each one's never rises while it's in flight, and the one on screen
        // never falls
        for (gap, slowdown) in [(0.04, 5.0), (0.04, 1.0), (0.01, 1.0), (0.02, 5.0), (0.1, 1.0)] {
            let (frames, step) = run(5, gap, slowdown);
            assert!(step < 1e-12, "gap {gap}, slowdown {slowdown}: a share moved {step} at a switch");
            for w in frames.windows(2) {
                let ((t0, on0, a), (_, on1, b)) = (&w[0], &w[1]);
                for tab in 1..=5u32 {
                    let (x, y) = (a[tab as usize - 1], b[tab as usize - 1]);
                    if tab != *on0 && tab != *on1 {
                        assert!(y <= x + 1e-12, "gap {gap}, slowdown {slowdown}: tab {tab} rose {x} to {y} after {t0}");
                    }
                    if tab == *on0 && tab == *on1 {
                        assert!(y >= x - 1e-12, "gap {gap}, slowdown {slowdown}: tab {tab} on screen fell {x} to {y} after {t0}");
                    }
                }
                let total: f64 = b.iter().sum();
                assert!((total - 1.0).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn a_switch_back_to_the_oldest_keeps_every_share() {
        // Five tabs 40 ms apart at slowdown 5, then back to the first, the oldest, on top of
        // three more: it comes out whole under the rest, and each newer one, which lay under it,
        // is played again from the opacity that keeps its share
        let curve = curve(5.0);
        let mut f = Flight::default();
        for (k, on) in [1, 2, 3, 4].into_iter().enumerate() {
            f.switch(Some(on), |_| false, 0.04 * k as f64, &curve, usize::MAX);
        }
        let t = 0.16;
        let before: Vec<f64> = (1..=4).map(|tab| shown(&f, t, Some(tab))).collect();
        let five = shown(&f, t, None);
        let s = f.switch(Some(5), |k| *k == 1, t, &curve, usize::MAX);
        assert_eq!(s.back.as_ref().map(|b| b.0), Some(1));
        // Every one that lay under it, and the tab left
        assert_eq!(s.redrive, vec![0, 1, 2, 3]);
        assert!((shown(&f, t, None) - before[0]).abs() < 1e-12, "one");
        for tab in 2..=4 {
            assert!((shown(&f, t, Some(tab)) - before[tab as usize - 1]).abs() < 1e-12, "{tab}");
        }
        assert!((shown(&f, t, Some(5)) - five).abs() < 1e-12);
        // Their opacity as played from now follows that, to well under a level
        for i in 0..f.len() {
            let seg = f.opacity_segments(i, t, STEP, EPSILON);
            for ms in 0..1100 {
                let tau = ms as f64 / 1000.0;
                let want = f.opacity(i, t + tau).0;
                assert!((seg.value(tau) - want).abs() < 1.0 / 255.0, "{i} at {tau}: {} for {want}", seg.value(tau));
            }
            assert_eq!(seg.end_value, 0.0);
            assert!((t + seg.end - f.leaving()[i].end()).abs() < 1e-9);
        }
    }

    #[test]
    fn a_switch_back_under_an_older_fade_changes_nothing_above_it() {
        // One to two to three, then back to two: one stays on top as it was, and only three (the
        // tab left) is played from now
        let curve = curve(1.0);
        let mut f = Flight::default();
        f.switch(Some(1), |_| false, 0.0, &curve, usize::MAX);
        f.switch(Some(2), |_| false, 0.02, &curve, usize::MAX);
        let one = f.opacity(0, 0.05);
        let s = f.switch(Some(3), |k| *k == 2, 0.05, &curve, usize::MAX);
        assert_eq!(s.redrive, vec![1]);
        assert_eq!(f.opacity(0, 0.05), one);
    }

    #[test]
    fn a_tab_that_showed_nothing_goes_at_once() {
        // Left the moment it came, under a stack covering it all: nothing of it to fade
        let curve = curve(1.0);
        let mut f = Flight::default();
        f.switch(Some(1), |_| false, 0.0, &curve, usize::MAX);
        let s = f.switch(Some(2), |_| false, 0.0, &curve, usize::MAX);
        assert_eq!(s.left, None);
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn past_its_depth_the_one_showing_least_goes() {
        // Four switches at real speed 40 ms apart into a stack three deep: the oldest has
        // nearly gone by then, so it goes, and every other keeps its share
        let curve = curve(1.0);
        let mut f = Flight::default();
        for (k, on) in [1, 2, 3].into_iter().enumerate() {
            f.switch(Some(on), |_| false, 0.04 * k as f64, &curve, 3);
        }
        let t = 0.12;
        let shares: Vec<f64> = (1..=3).map(|tab| shown(&f, t, Some(tab))).collect();
        let s = f.switch(Some(4), |_| false, t, &curve, 3);
        assert_eq!(s.dropped.len(), 1);
        assert_eq!(s.dropped[0].0, 1);
        assert!((s.dropped[0].1 - shares[0]).abs() < 1e-12);
        assert!(shares[0] < 0.02, "the oldest showed {}", shares[0]);
        assert_eq!(f.len(), 3);
        assert_eq!(s.redrive, vec![0, 1, 2]);
        for tab in 2..=3 {
            assert!((shown(&f, t, Some(tab)) - shares[tab as usize - 1]).abs() < 1e-12);
        }
    }

    /// The most a switch every `gap` seconds through new tabs at real speed puts away past
    /// `depth`, of what it showed, and the most in flight showing a level of light or more
    fn steady(gap: f64, depth: usize) -> (f64, usize) {
        let curve = curve(1.0);
        let mut f = Flight::default();
        let (mut dropped, mut seen) = (0.0_f64, 0);
        for k in 0..200u32 {
            let t = gap * k as f64;
            f.put_away_done(t);
            let s = f.switch(Some(k), |_| false, t, &curve, depth);
            dropped = s.dropped.iter().map(|(_, share)| *share).fold(dropped, f64::max);
            seen = seen.max(f.leaving().iter().filter(|l| l.share(t).0 >= 1.0 / 255.0).count());
        }
        (dropped, seen)
    }

    #[test]
    fn past_its_depth_nothing_seen_goes_at_a_persons_rates() {
        // Side buttons or the rail's pads 40 ms apart, held keys 33 ms, and faster
        for (gap, most) in [(0.1, 2), (0.04, 4), (0.033, 5), (0.025, 6)] {
            let (dropped, seen) = steady(gap, WORDS_DEPTH);
            assert!(dropped < 1.0 / 255.0, "{gap}: {dropped}");
            assert!(seen <= most, "{gap}: {seen} showing a level or more");
        }
        // Nothing at all goes 40 ms apart
        assert_eq!(steady(0.04, WORDS_DEPTH).0, 0.0);
        // Posted every frame, 60 and 160 a second: what goes shows a little, the step it makes
        let (sixty, _) = steady(1.0 / 60.0, WORDS_DEPTH);
        let (one_sixty, _) = steady(1.0 / 160.0, WORDS_DEPTH);
        assert!(sixty > 0.02 && sixty < 0.025, "{sixty}");
        assert!(one_sixty > 0.085 && one_sixty < 0.095, "{one_sixty}");
    }

    #[test]
    fn on_a_spring_the_newest_can_be_done_first() {
        // A run of switches with a spring for the curve, as `lbterm_motion.swap` may be set: the
        // tab left last goes in from less (what the stack above didn't cover) and a spring
        // settles sooner from less, so it's done before one left before it. What draws the
        // newest's words mustn't put the older ones away when it's done (wezterm-gui's
        // `prepare_swap`): until 2026-10-31 it did, and their words left the strip at once
        // Two switches 20 ms apart: the second tab goes in at about 0.13
        let spring = "spring(200ms, 0)".parse::<Curve>().unwrap();
        let mut f = Flight::default();
        f.switch(Some(1), |_| false, 0.0, &spring, usize::MAX);
        f.switch(Some(2), |_| false, 0.02, &spring, usize::MAX);
        let ends: Vec<f64> = f.leaving().iter().map(|l| l.end()).collect();
        assert!(ends[1] < ends[0], "the newest ends at {}, the one before at {}", ends[1], ends[0]);
        // When it's done the older one still shows, and goes on by itself
        let t = (ends[1] + ends[0]) / 2.0;
        assert_eq!(f.put_away_done(t), vec![2]);
        assert_eq!(f.len(), 1);
        assert!(f.leaving()[0].share(t).0 > 0.0);
        // With tab_switch's timed curve the newest ends last, as it was built for
        let curve = curve(1.0);
        let mut f = Flight::default();
        for (k, on) in [1, 2, 3].into_iter().enumerate() {
            f.switch(Some(on), |_| false, 0.04 * k as f64, &curve, usize::MAX);
        }
        let ends: Vec<f64> = f.leaving().iter().map(|l| l.end()).collect();
        assert!(ends.windows(2).all(|w| w[0] <= w[1]), "{ends:?}");
    }

    #[test]
    fn a_tab_come_back_from_the_only_one_in_flight_rises_on_the_curve() {
        // Back and forth: what the tab come back shows rises on tab_switch itself, from where it
        // was, so its outline, which rises with it (wezterm-gui's `switch_outline`), does as before
        let curve = curve(5.0);
        let mut f = Flight::default();
        f.switch(Some(1), |_| false, 0.0, &curve, usize::MAX);
        let t = 0.3;
        f.switch(Some(2), |k| *k == 1, t, &curve, usize::MAX);
        let up = f.uncovered_segments(t, STEP, EPSILON);
        let from = up.value(0.0);
        let m = Motion::new(0.0, 1.0, 0.0, curve.clone());
        for ms in 0..1000 {
            let tau = ms as f64 / 1000.0;
            let come = (up.value(tau) - from) / (1.0 - from);
            assert!((come - m.value(tau)).abs() < 0.004, "at {tau}: {come} for {}", m.value(tau));
        }
    }

    #[test]
    fn what_a_tab_come_back_shows_rises_to_all_of_it() {
        let curve = curve(5.0);
        let mut f = Flight::default();
        for (k, on) in [1, 2, 3].into_iter().enumerate() {
            f.switch(Some(on), |_| false, 0.04 * k as f64, &curve, usize::MAX);
        }
        let t = 0.12;
        f.switch(Some(4), |k| *k == 1, t, &curve, usize::MAX);
        let seg = f.uncovered_segments(t, STEP, EPSILON);
        assert!((seg.value(0.0) - shown(&f, t, None)).abs() < 1e-3);
        let mut last = 0.0;
        for ms in 0..1200 {
            let tau = ms as f64 / 1000.0;
            let want = shown(&f, t + tau, None);
            assert!((seg.value(tau) - want).abs() < 1.0 / 255.0, "at {tau}");
            assert!(want >= last - 1e-12);
            last = want;
        }
        assert_eq!(seg.end_value, 1.0);
    }
}
