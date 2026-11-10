//! When a suggested tab connection's light runs along its line (lbterm/LINKS.md, "Connections").
//! Lachlan's 1a (2026-11-01): it runs [`LAPS`] laps each time a suggestion comes onto the rail,
//! each time the file changes what it says of one, and each time the pointer comes onto the rail,
//! and between them it's still, glowing at the line's middle. Until then it ran for as long as the
//! suggestion showed, which on his rail is hours, and kept DWM composing every refresh of the
//! 160 Hz screen all that time (lbterm/COMPOSITOR.md, "Loops").
//!
//! The laps go by the loops' rule too (`work::loops_play`): while the window can't be seen, or
//! with motion off, the light is still, and laps cut off that way are owed, run again from their
//! beginning once it can be seen. The compositor gives DWM what [`Laps::update`] says
//! (wezterm-gui's compositor.rs, `ConnectionLamps`), and the state file's `loops` names the light
//! only while its laps run ([`Laps::running`]).

use std::time::{Duration, Instant};

/// How many laps the light runs each time
pub const LAPS: u32 = 3;

/// One suggested line on the rail, as a paint finds it: the connection's id, a key of what the
/// file says of it (its members, its why, who found it and when, when it came to stand so), a key
/// of where its light goes (its path, its template), and its lap's seconds
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line<'a> {
    pub id: &'a str,
    pub says: u64,
    pub place: u64,
    pub lap: f64,
}

/// What a line's light does
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Glow {
    /// Still at the line's middle
    Still,
    /// `laps` laps from `from`, each from the middle and back to it, then still there
    Laps { from: Instant, laps: u32 },
}

impl Glow {
    /// When its laps end, a lap being `lap` seconds
    pub fn until(&self, lap: f64) -> Option<Instant> {
        match *self {
            Glow::Still => None,
            Glow::Laps { from, laps } => Some(from + Duration::from_secs_f64(lap.max(0.0) * laps as f64)),
        }
    }

    /// Whether its laps are running at `now`
    pub fn running(&self, lap: f64, now: Instant) -> bool {
        self.until(lap).is_some_and(|until| until > now)
    }

    /// Whether DWM, given `self` and now asked for `want`, shows the same: laps that have ended
    /// left the light at the middle, where still is, so no new visual need be made for it (the
    /// compositor then sets the middle on the same visual as a value, letting the ended
    /// animation go: `settle_lamps` in wezterm-gui's compositor.rs)
    pub fn shows_as(&self, want: &Glow, lap: f64, now: Instant) -> bool {
        self == want || (matches!(want, Glow::Still) && !self.running(lap, now))
    }
}

#[derive(Debug)]
struct Kept {
    id: String,
    says: u64,
    place: u64,
    lap: f64,
    /// Laps to run once they can be: it came, it changed, the pointer came onto the rail, or its
    /// laps were cut off with the window unseen
    owed: bool,
    glow: Glow,
}

/// Every suggested line's light on one rail
#[derive(Debug, Default)]
pub struct Laps {
    lines: Vec<Kept>,
    /// Whether loops played at the last [`Self::update`]
    play: bool,
}

impl Laps {
    /// The pointer came onto the rail at `now`: every light that's still (its laps done or none
    /// run) runs its laps (one running goes on as it is). Whether that owes any laps not owed
    /// before, so a frame should come to start them. With loops not playing at the last update
    /// (motion off, or the window unseen, where no pointer comes) it owes nothing and asks for
    /// nothing: a frame would start nothing, and laps owed then would run whenever motion came
    /// back on, long after the pointer came (PITFALLS 66)
    pub fn pointer_came(&mut self, now: Instant) -> bool {
        if !self.play {
            return false;
        }
        let mut any = false;
        for line in &mut self.lines {
            if !line.owed && !line.glow.running(line.lap, now) {
                line.owed = true;
                any = true;
            }
        }
        any
    }

    /// Whether any light is owed its laps: a frame with loops playing would start them
    pub fn owed(&self) -> bool {
        self.lines.iter().any(|k| k.owed)
    }

    /// The rail's suggested lines now, in their order, and whether loops play
    /// (`work::loops_play`: motion on and the window seen): what each one's light does now.
    /// A line not here before runs its laps, as does one whose file entry changed; one gone is
    /// forgotten, so if it's suggested again it runs them again. Laps still to run when its place
    /// changes (rows moved, the window resized) go on along the new place from its middle, as
    /// many whole laps as were left
    pub fn update(&mut self, lines: &[Line], play: bool, now: Instant) -> Vec<Glow> {
        self.play = play;
        let mut old = std::mem::take(&mut self.lines);
        let mut out = Vec::with_capacity(lines.len());
        for line in lines {
            let mut kept = match old.iter().position(|k| k.id == line.id) {
                Some(i) => {
                    let mut k = old.swap_remove(i);
                    if k.says != line.says {
                        k.owed = true;
                    }
                    k
                }
                None => Kept {
                    id: line.id.to_string(),
                    says: line.says,
                    place: line.place,
                    lap: line.lap,
                    owed: true,
                    glow: Glow::Still,
                },
            };
            kept.says = line.says;
            if let Some(until) = kept.glow.until(kept.lap) {
                if until <= now {
                    kept.glow = Glow::Still;
                } else if !play {
                    // Cut off: run again once the window can be seen
                    kept.glow = Glow::Still;
                    kept.owed = true;
                } else if kept.place != line.place || kept.lap != line.lap {
                    let left = until.duration_since(now).as_secs_f64();
                    let laps = (left / line.lap.max(1e-3)).ceil().max(1.0) as u32;
                    kept.glow = Glow::Laps { from: now, laps };
                }
            }
            kept.place = line.place;
            kept.lap = line.lap;
            if kept.owed && play {
                kept.owed = false;
                if matches!(kept.glow, Glow::Still) {
                    kept.glow = Glow::Laps { from: now, laps: LAPS };
                }
            }
            out.push(kept.glow);
            self.lines.push(kept);
        }
        out
    }

    /// Whether any light's laps are running at `now`
    pub fn running(&self, now: Instant) -> bool {
        self.lines.iter().any(|k| k.glow.running(k.lap, now))
    }

    /// When the last laps running at `now` end
    pub fn until(&self, now: Instant) -> Option<Instant> {
        self.lines
            .iter()
            .filter_map(|k| k.glow.until(k.lap))
            .filter(|&u| u > now)
            .max()
    }
}

/// The light's laps for DWM: straight from each of `ys` (where it is evenly through one lap,
/// starting at the middle) to the next, `laps` times over, and the middle once they're done.
/// Each segment's start in seconds, its value and its slope a second, and when the last ends
pub fn segments(ys: &[f32], lap: f64, laps: u32) -> (Vec<(f64, f32, f32)>, f64) {
    let n = ys.len();
    if n == 0 || laps == 0 || lap <= 0.0 {
        return (vec![], 0.0);
    }
    let dt = lap / n as f64;
    let mut out = Vec::with_capacity(n * laps as usize);
    for l in 0..laps as usize {
        for (k, &y) in ys.iter().enumerate() {
            let next = ys[(k + 1) % n];
            out.push(((l * n + k) as f64 * dt, y, ((next - y) as f64 / dt) as f32));
        }
    }
    (out, lap * laps as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: &str, says: u64, place: u64, lap: f64) -> Line<'_> {
        Line { id, says, place, lap }
    }
    fn s(n: f64) -> Duration {
        Duration::from_secs_f64(n)
    }

    #[test]
    fn a_suggestion_coming_onto_the_rail_runs_its_laps_then_holds_still() {
        let t0 = Instant::now();
        let mut laps = Laps::default();
        let lines = [line("c-pair", 1, 1, 2.0)];
        assert_eq!(laps.update(&lines, true, t0), vec![Glow::Laps { from: t0, laps: LAPS }]);
        assert!(laps.running(t0));
        assert_eq!(laps.until(t0), Some(t0 + s(2.0 * LAPS as f64)));
        // Frames on the way change nothing, so DWM's laps carry on
        assert_eq!(laps.update(&lines, true, t0 + s(1.0)), vec![Glow::Laps { from: t0, laps: LAPS }]);
        // Once they're done it's still, and stays so frame after frame
        let after = t0 + s(2.0 * LAPS as f64);
        assert!(!laps.running(after));
        assert_eq!(laps.until(after), None);
        assert_eq!(laps.update(&lines, true, after), vec![Glow::Still]);
        assert_eq!(laps.update(&lines, true, after + s(3600.0)), vec![Glow::Still]);
        assert!(!laps.running(after + s(3600.0)));
    }

    #[test]
    fn the_file_changing_what_it_says_of_one_runs_its_laps_again() {
        let t0 = Instant::now();
        let mut laps = Laps::default();
        laps.update(&[line("c-pair", 1, 1, 2.0)], true, t0);
        let later = t0 + s(60.0);
        assert_eq!(laps.update(&[line("c-pair", 1, 1, 2.0)], true, later), vec![Glow::Still]);
        assert_eq!(laps.update(&[line("c-pair", 2, 1, 2.0)], true, later), vec![Glow::Laps { from: later, laps: LAPS }]);
        // A change while they run is lit already: they go on as they are
        let mid = later + s(1.0);
        assert_eq!(laps.update(&[line("c-pair", 3, 1, 2.0)], true, mid), vec![Glow::Laps { from: later, laps: LAPS }]);
        assert_eq!(laps.update(&[line("c-pair", 3, 1, 2.0)], true, later + s(6.0)), vec![Glow::Still]);
    }

    #[test]
    fn the_pointer_coming_onto_the_rail_runs_every_still_light_again() {
        let t0 = Instant::now();
        let mut laps = Laps::default();
        let both = [line("a", 1, 1, 2.0), line("b", 1, 2, 4.0)];
        laps.update(&both, true, t0);
        // b's laps run longer: at 7 s a's are done and b's still running
        let t1 = t0 + s(7.0);
        assert_eq!(laps.update(&both, true, t1), vec![Glow::Still, Glow::Laps { from: t0, laps: LAPS }]);
        assert!(laps.pointer_came(t1), "a is still, so it's owed");
        assert!(laps.owed());
        assert_eq!(laps.update(&both, true, t1), vec![Glow::Laps { from: t1, laps: LAPS }, Glow::Laps { from: t0, laps: LAPS }]);
        assert!(!laps.owed());
        // With every light running, the pointer owes nothing
        assert!(!laps.pointer_came(t1));
        // Laps that have ended with no update since (the compositor leaves a frame with nothing
        // new alone): the clock says they're done, so the pointer runs them again
        let t2 = t1 + s(13.0);
        assert!(laps.pointer_came(t2), "both done by the clock");
        assert_eq!(laps.update(&both, true, t2), vec![Glow::Laps { from: t2, laps: LAPS }; 2]);
        // Nothing on the rail, nothing owed
        let mut none = Laps::default();
        assert!(!none.pointer_came(t0));
        assert_eq!(none.update(&[], true, t0), vec![]);
        assert!(!none.running(t0));
        assert!(!none.pointer_came(t0));
        assert!(!none.owed());
    }

    #[test]
    fn with_loops_not_playing_the_pointer_asks_for_nothing_and_owes_nothing() {
        let t0 = Instant::now();
        let mut laps = Laps::default();
        let lines = [line("c-pair", 1, 1, 2.0)];
        laps.update(&lines, true, t0);
        let done = t0 + s(6.0);
        assert_eq!(laps.update(&lines, true, done), vec![Glow::Still]);
        // Motion off (or the window unseen): the pointer onto the rail, twice, asks for no frame,
        // which would start nothing, and owes nothing
        assert_eq!(laps.update(&lines, false, done + s(1.0)), vec![Glow::Still]);
        assert!(!laps.pointer_came(done + s(2.0)));
        assert!(!laps.pointer_came(done + s(3.0)));
        assert!(!laps.owed());
        // Motion on again: nothing runs for a pointer that came while it was off
        assert_eq!(laps.update(&lines, true, done + s(60.0)), vec![Glow::Still]);
        // Playing, the pointer owes laps once, and coming again before the frame that starts
        // them asks for nothing more
        assert!(laps.pointer_came(done + s(61.0)));
        assert!(laps.owed());
        assert!(!laps.pointer_came(done + s(61.5)), "owed already: no second frame asked for");
        assert_eq!(laps.update(&lines, true, done + s(62.0)), vec![Glow::Laps { from: done + s(62.0), laps: LAPS }]);
        assert!(!laps.owed());
    }

    #[test]
    fn laps_wait_while_the_window_cant_be_seen_and_cut_off_run_again_from_the_start() {
        let t0 = Instant::now();
        let mut laps = Laps::default();
        let lines = [line("c-pair", 1, 1, 2.0)];
        // Suggested while minimized: still, owed
        assert_eq!(laps.update(&lines, false, t0), vec![Glow::Still]);
        assert!(!laps.running(t0));
        let seen = t0 + s(30.0);
        assert_eq!(laps.update(&lines, true, seen), vec![Glow::Laps { from: seen, laps: LAPS }]);
        // Minimized half way: still at once, and from the start again once seen
        let hid = seen + s(3.0);
        assert_eq!(laps.update(&lines, false, hid), vec![Glow::Still]);
        assert!(!laps.running(hid));
        let back = hid + s(10.0);
        assert_eq!(laps.update(&lines, true, back), vec![Glow::Laps { from: back, laps: LAPS }]);
        // Done, then minimized and restored: nothing owed, nothing runs
        let done = back + s(7.0);
        assert_eq!(laps.update(&lines, true, done), vec![Glow::Still]);
        assert_eq!(laps.update(&lines, false, done + s(1.0)), vec![Glow::Still]);
        assert_eq!(laps.update(&lines, true, done + s(2.0)), vec![Glow::Still]);
        // The pointer onto the rail, then the window unseen before a frame started its laps:
        // they're owed for when it can be seen
        assert!(laps.pointer_came(done + s(2.5)));
        assert_eq!(laps.update(&lines, false, done + s(3.0)), vec![Glow::Still]);
        assert!(laps.owed());
        assert_eq!(laps.update(&lines, true, done + s(4.0)), vec![Glow::Laps { from: done + s(4.0), laps: LAPS }]);
    }

    #[test]
    fn a_line_that_moves_while_its_laps_run_goes_on_from_its_new_middle_for_the_laps_left() {
        let t0 = Instant::now();
        let mut laps = Laps::default();
        laps.update(&[line("c-pair", 1, 1, 2.0)], true, t0);
        // 2.5 s in, 3.5 s of laps left: two whole laps of the new line's 2 s, from its middle
        let moved = t0 + s(2.5);
        assert_eq!(laps.update(&[line("c-pair", 1, 2, 2.0)], true, moved), vec![Glow::Laps { from: moved, laps: 2 }]);
        // Longer now (a lap of 5 s): what's left is under one lap, so one
        let longer = moved + s(1.0);
        assert_eq!(laps.update(&[line("c-pair", 1, 3, 5.0)], true, longer), vec![Glow::Laps { from: longer, laps: 1 }]);
        // A still light moving stays still
        let done = longer + s(5.0);
        assert_eq!(laps.update(&[line("c-pair", 1, 3, 5.0)], true, done), vec![Glow::Still]);
        assert_eq!(laps.update(&[line("c-pair", 1, 4, 5.0)], true, done), vec![Glow::Still]);
    }

    #[test]
    fn a_suggestion_gone_and_back_runs_its_laps_again() {
        let t0 = Instant::now();
        let mut laps = Laps::default();
        laps.update(&[line("c-pair", 1, 1, 2.0)], true, t0);
        let t1 = t0 + s(10.0);
        assert_eq!(laps.update(&[line("c-pair", 1, 1, 2.0)], true, t1), vec![Glow::Still]);
        // Accepted, say: gone from the suggestions, its laps forgotten
        assert_eq!(laps.update(&[], true, t1), vec![]);
        assert_eq!(laps.update(&[line("c-pair", 1, 1, 2.0)], true, t1), vec![Glow::Laps { from: t1, laps: LAPS }]);
        // A second one coming leaves the first's laps as they are
        let t2 = t1 + s(1.0);
        let out = laps.update(&[line("c-pair", 1, 1, 2.0), line("c-new", 1, 2, 2.0)], true, t2);
        assert_eq!(out, vec![Glow::Laps { from: t1, laps: LAPS }, Glow::Laps { from: t2, laps: LAPS }]);
    }

    #[test]
    fn laps_that_have_ended_show_as_still_so_nothing_is_given_dwm_again() {
        let t0 = Instant::now();
        let ran = Glow::Laps { from: t0, laps: 3 };
        assert!(!ran.shows_as(&Glow::Still, 2.0, t0 + s(5.0)), "running: still is a change");
        assert!(ran.shows_as(&Glow::Still, 2.0, t0 + s(6.0)));
        assert!(ran.shows_as(&ran, 2.0, t0));
        assert!(Glow::Still.shows_as(&Glow::Still, 2.0, t0));
        assert!(!Glow::Still.shows_as(&ran, 2.0, t0));
        assert!(!ran.shows_as(&Glow::Laps { from: t0 + s(1.0), laps: 3 }, 2.0, t0 + s(9.0)));
    }

    #[test]
    fn its_segments_are_whole_laps_from_the_middle_and_back_to_it() {
        let ys = [10.0, 20.0, 10.0, 0.0];
        let (segs, end) = segments(&ys, 2.0, 3);
        assert_eq!(segs.len(), 12);
        assert!((end - 6.0).abs() < 1e-9);
        assert_eq!(segs[0], (0.0, 10.0, 20.0));
        assert_eq!(segs[3], (1.5, 0.0, 20.0), "the lap's last runs back up to the middle");
        assert_eq!(segs[4].0, 2.0);
        assert_eq!(segs[11].1, 0.0);
        // Each segment ends where the next begins
        for w in segs.windows(2) {
            let (t, v, slope) = w[0];
            assert!((v + slope * (w[1].0 - t) as f32 - w[1].1).abs() < 1e-4, "{:?}", w);
        }
        let last = segs[11];
        assert!((last.1 + last.2 * (end - last.0) as f32 - ys[0]).abs() < 1e-4, "it ends at the middle");
        assert_eq!(segments(&ys, 2.0, 0), (vec![], 0.0));
        assert_eq!(segments(&[], 2.0, 3), (vec![], 0.0));
    }
}
