//! What a frame would draw into one visual, as one number, so a swapchain that already shows it
//! is left as it is (lbterm/COMPOSITOR.md, "Only what changed is drawn").
//!
//! Every visual is a swapchain, and before this every frame drew and presented every one of them
//! whatever changed: a window with four panes and the rail has 24, and the InMail button's count
//! changing, which only the strip shows, drew and presented all 24, about 10 ms of the GUI thread
//! of a frame's 20. Now each quad's vertices are folded into a number as they're written
//! (wezterm-gui's quad.rs), a visual's key folds in the quads it would be given in the order it
//! would draw them and everything else that decides its pixels, and a visual whose swapchain
//! shows that key already isn't drawn or presented: DWM keeps showing its last frame. A
//! swapchain made or resized shows nothing known, so it's always drawn.

use crate::PixelRect;

/// Folds `word` into `hash`. For one word each step is a bijection on the hash, so two runs of
/// words of one length that differ in a single word always end apart.
#[inline]
pub fn fold(hash: &mut u64, word: u64) {
    *hash = (hash.rotate_left(5) ^ word).wrapping_mul(0x517c_c1b7_2722_0a95);
}

/// Two `f32`s, bit for bit, as one word
#[inline]
pub fn pair(a: f32, b: f32) -> u64 {
    (a.to_bits() as u64) << 32 | b.to_bits() as u64
}

/// `values` folded into `hash` two at a time, bit for bit, an odd last one alone
#[inline]
pub fn fold_f32s(hash: &mut u64, values: &[f32]) {
    let mut pairs = values.chunks_exact(2);
    for two in &mut pairs {
        fold(hash, pair(two[0], two[1]));
    }
    for &last in pairs.remainder() {
        fold(hash, last.to_bits() as u64);
    }
}

/// What `moved` folds in before the offset, so a moved hash isn't the fold of a plain word
const MOVED: u64 = 0x6c62_7465_726d_0003;

/// The hash of a quad drawn `dy` pixels below where it was laid out, from the hash it was laid
/// out with. A pane's line is laid out as if it were at the window's top and kept that way, by
/// what it shows rather than where (wezterm-gui's render/pane.rs, the line quad cache), and it's
/// moved to its row as it's copied into the layer (`place`). So that the compositor's content keys
/// and the GPU copy's dirty runs see a line that moved, its quads' hashes move with it: for one
/// `dy` this is a bijection on the hashes (`fold`), so quads that differ still differ moved, and
/// for one quad two offsets give two hashes, so a line moved a row changes every hash it has.
#[inline]
pub fn moved(hash: u64, dy: f32) -> u64 {
    let mut moved = hash;
    fold(&mut moved, MOVED ^ dy.to_bits() as u64);
    moved
}

/// Quads kept laid out at row 0, `from` (`per_quad` vertices each) with their `hashes`, written
/// into `into` and `into_hashes` `dy` pixels lower: `down` moves one vertex down by `dy`, and each
/// hash is `moved`. As many quads as both sides hold; the number written. One pass over the line's
/// vertices, each moved as it's copied and `into` only written, never read: under the OpenGL front
/// end `into` is a mapped GL buffer, slow to read back (lbterm/COMPOSITOR.md). Every frame built
/// every quad's four vertices again from what it was given before (wezterm-gui's quad.rs)
pub fn place<V: Copy>(
    into: &mut [V],
    into_hashes: &mut [u64],
    from: &[V],
    hashes: &[u64],
    per_quad: usize,
    dy: f32,
    down: impl Fn(&mut V, f32),
) -> usize {
    let quads = hashes
        .len()
        .min(into_hashes.len())
        .min(from.len() / per_quad)
        .min(into.len() / per_quad);
    let vertices = quads * per_quad;
    for (to, from) in into[..vertices].iter_mut().zip(&from[..vertices]) {
        let mut v = *from;
        down(&mut v, dy);
        *to = v;
    }
    for (to, &hash) in into_hashes[..quads].iter_mut().zip(hashes) {
        *to = moved(hash, dy);
    }
    quads
}

/// The key of what one visual is drawn from this frame, built up in the order it's drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key(u64);

impl Key {
    /// A visual at `rect` (its texture's size, and where its projection puts it in the window),
    /// with `settings`: what else decides its pixels (the glyph atlas, the colour transform, the
    /// colour it's cleared to)
    pub fn new(rect: PixelRect, settings: &[u64]) -> Self {
        let mut hash = 0x6c62_7465_726d_0001;
        fold(&mut hash, (rect.x as u32 as u64) << 32 | rect.y as u32 as u64);
        fold(&mut hash, (rect.w as u64) << 32 | rect.h as u64);
        fold(&mut hash, settings.len() as u64);
        for &s in settings {
            fold(&mut hash, s);
        }
        Self(hash)
    }

    /// The quads `ranges` of z-layer `zindex`'s vertex buffer `sub` are drawn into it next, each
    /// quad's own hash in `hashes`. How the quads are split into draws doesn't change what's
    /// drawn, so only which quads, in order, counts.
    pub fn quads(&mut self, zindex: i8, sub: usize, ranges: &[std::ops::Range<usize>], hashes: &[u64]) {
        let count: usize = ranges.iter().map(|r| r.len()).sum();
        if count == 0 {
            return;
        }
        fold(&mut self.0, (zindex as u8 as u64) << 40 | (sub as u64) << 32 | count as u64);
        for range in ranges {
            for q in range.clone() {
                // A quad past what was hashed (more than the buffer held: the pass is painted
                // again with more) can't be told apart, so it never matches
                fold(&mut self.0, hashes.get(q).copied().unwrap_or(0x9e37_79b9_7f4a_7c15 ^ q as u64));
            }
        }
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

/// Whether a swapchain that shows `shows` (nothing known: just made or resized) has to be drawn
/// and presented for a frame that would draw `wants` into it (nothing: draw it whatever it shows)
pub fn must_draw(shows: Option<u64>, wants: Option<u64>) -> bool {
    wants.is_none() || shows != wants
}

/// Runs of quads closer than this are written to the GPU as one: each write is a buffer of its
/// own that wgpu makes and copies from, so a few quads written for nothing cost less than another
pub const WRITE_GAP: usize = 32;
/// Past this many runs, one write from the first to the last
pub const WRITE_RUNS: usize = 16;

/// Which of a layer's quads its GPU copy has to be written with, as runs of quad indexes: those
/// whose hash (`now`, this frame's) differs from the hash of what the copy holds there (`held`, the
/// hashes as they were when it was last written), and those past what it held. Runs less than
/// `gap` apart are one, and past `max_runs` runs it's one from the first to the last. A pane's
/// lines that didn't change, a spinner going round on one row, the scrollbar's thumb: each frame
/// writes what changed, where it wrote every quad of every layer before.
pub fn dirty_runs(now: &[u64], held: &[u64], gap: usize, max_runs: usize) -> Vec<std::ops::Range<usize>> {
    let mut runs: Vec<std::ops::Range<usize>> = vec![];
    for (i, hash) in now.iter().enumerate() {
        if held.get(i) == Some(hash) {
            continue;
        }
        match runs.last_mut() {
            Some(last) if i - last.end < gap => last.end = i + 1,
            _ => runs.push(i..i + 1),
        }
    }
    if runs.len() > max_runs {
        let (start, end) = (runs[0].start, runs[runs.len() - 1].end);
        runs = vec![start..end];
    }
    runs
}

/// How many quads a layer that held `held` and needs `need` (rounded up to a multiple of 128) is
/// made to hold: half as many again as it held at least, so one filling up a little at a time
/// is made again a few times, not every 128 quads, when only what a frame uses is written to the
/// GPU and holding more costs only memory
pub fn grow(held: usize, need: usize) -> usize {
    let need = (need + 127) & !127;
    need.max((held + held / 2 + 127) & !127)
}

/// The parts of a draw's uniform the shader reads (wezterm-gui's shader.wgsl: the text's colour
/// transform `hsb`, then the `projection`; not `milliseconds`, which it doesn't), bit for bit: a
/// bind group made for one key serves every draw with it (wezterm-gui's termwindow/gpu_work.rs),
/// where every draw made a uniform buffer and its bind group of its own
pub fn uniform_key(hsb: [f32; 3], projection: &[[f32; 4]; 4]) -> [u32; 19] {
    let mut key = [0u32; 19];
    for (i, v) in hsb.iter().chain(projection.iter().flatten()).enumerate() {
        key[i] = v.to_bits();
    }
    key
}

/// What every uniform a window draws with in a frame shares, bit for bit: the window's size, whose
/// halves each visual's projection is made from (wezterm-gui's render/composited.rs), and the
/// text's colour transform. A uniform made under one can be drawn with again only once the window
/// is back under it
pub fn uniforms_at(width: u32, height: u32, hsb: [f32; 3]) -> [u32; 5] {
    [width, height, hsb[0].to_bits(), hsb[1].to_bits(), hsb[2].to_bits()]
}

/// Which `uniforms_at` a window keeps uniforms for (wezterm-gui's termwindow/gpu_work.rs): the one
/// it's drawn under now and the one before, so a maximize and its restore, or a size dragged back
/// across where it was, make nothing again. Every other one's go the frame the window is drawn
/// under a third: each held a buffer, which D3D12 places on 64 KB of the GPU's memory however
/// small, and none could be drawn with again till the window went back. Kept by age, a window
/// resized a pixel at a time 150 times kept 1,684 of them, 105 MB (lbterm/PITFALLS.md, trap 112)
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UniformsAt {
    now: Option<[u32; 5]>,
    before: Option<[u32; 5]>,
}

impl UniformsAt {
    /// The window drawn under `at`: the one whose uniforms go now, if any
    pub fn drawn_at(&mut self, at: [u32; 5]) -> Option<[u32; 5]> {
        if self.now == Some(at) {
            return None;
        }
        if self.before == Some(at) {
            std::mem::swap(&mut self.now, &mut self.before);
            return None;
        }
        let gone = self.before;
        self.before = self.now;
        self.now = Some(at);
        gone
    }

    /// What the window is drawn under now, which a uniform made or drawn with now is kept under
    pub fn now(&self) -> Option<[u32; 5]> {
        self.now
    }
}

/// Which uniforms a window keeps once it keeps more than `most` (wezterm-gui's
/// termwindow/gpu_work.rs, config's `GPU_UNIFORMS_KEPT`), from the frame each was last drawn in
/// (`used`): those drawn in the frame this gives or since, the newest `most / 2` and any others
/// drawn in that same frame. None while there are no more than `most`, so none goes. A bind group
/// let go while a frame on the GPU still draws with it stays till that frame is done (wgpu holds
/// it), so this bounds what the kept ones hold, never what a frame draws
pub fn uniforms_kept_since(used: impl ExactSizeIterator<Item = u64>, most: usize) -> Option<u64> {
    if used.len() <= most {
        return None;
    }
    let keep = (most / 2).max(1);
    let mut used: Vec<u64> = used.collect();
    let at = used.len() - keep;
    Some(*used.select_nth_unstable(at).1)
}

/// How many times Windows has said a setting changed, as far as a window's motion settings go:
/// with `enabled = 'system'` they read Windows' Animation effects, so each change can change
/// them; 'on' and 'off' read nothing of Windows', so for them it's always 0
/// (wezterm-gui's compositor.rs, `MotionCache`)
pub fn settings_seen(enabled: &str, settings_changes: u64) -> u64 {
    match enabled {
        "on" | "off" => 0,
        _ => settings_changes,
    }
}

/// Whether motion settings made earlier hold now: made from the config asked about now
/// (`same_config`), and when Windows' settings had changed as often as they have now (`made`,
/// `now`, each `settings_seen`). Otherwise they're made again: twenty curves parsed from their
/// CSS, which every paint did seven times over before they were kept
pub fn motion_holds(same_config: bool, made: u64, now: u64) -> bool {
    same_config && made == now
}

/// What a frame made and wrote on the GPU and how long its draw took, for the state file
/// (lbterm/STATE.md, `visuals`; wezterm-gui's termwindow/gpu_work.rs counts it)
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Work {
    /// Buffers and bind groups made
    pub made: usize,
    /// Vertex buffers written, and how many bytes
    pub uploads: usize,
    pub upload_bytes: usize,
    /// Render passes recorded and the submits they went in, and in microseconds how long
    /// gathering what each visual draws took (the layers' GPU copies written with it), recording
    /// the passes, and finishing and submitting them
    pub passes: usize,
    pub submits: usize,
    pub upload_us: u64,
    pub record_us: u64,
    pub submit_us: u64,
    /// Quads a vertex buffer's GPU copy wasn't written with because their hashes matched what it
    /// held, though their vertices weren't what it holds: a write to a quad that its hash doesn't
    /// fold in. Counted only when a test window asks (LBTERM_CHECK_UPLOADS), and always 0
    pub stale: usize,
}

impl Work {
    /// `n` buffers or bind groups made
    pub fn made(&mut self, n: usize) {
        self.made += n;
    }

    /// A vertex buffer written, `bytes` of it
    pub fn uploaded(&mut self, bytes: usize) {
        self.uploads += 1;
        self.upload_bytes += bytes;
    }

    /// A frame's drawing: `passes` render passes in `submits` submits, and how long gathering what
    /// each visual draws, recording the passes, and finishing and submitting them took
    pub fn recorded(&mut self, passes: usize, submits: usize, [upload, record, submit]: [std::time::Duration; 3]) {
        self.passes += passes;
        self.submits += submits;
        self.upload_us += upload.as_micros() as u64;
        self.record_us += record.as_micros() as u64;
        self.submit_us += submit.as_micros() as u64;
    }
}

/// Quads in `0..quads` whose `now` and `held` hashes are the same but whose vertices, `now_vertices`
/// against `held_vertices` (`per_quad` of them a quad), aren't: what `dirty_runs` leaves out of a
/// write that it shouldn't have. A quad's hash covers every byte of its vertices, so there are
/// none; a write the hash doesn't fold in makes one, and the GPU keeps showing what it held
pub fn stale_quads<V: PartialEq>(
    now: &[u64],
    held: &[u64],
    now_vertices: &[V],
    held_vertices: &[V],
    per_quad: usize,
    quads: usize,
) -> usize {
    (0..quads)
        .filter(|&q| now.get(q).is_some() && now.get(q) == held.get(q))
        .filter(|&q| {
            let range = q * per_quad..(q + 1) * per_quad;
            match (now_vertices.get(range.clone()), held_vertices.get(range)) {
                (Some(a), Some(b)) => a != b,
                _ => false,
            }
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(rect: PixelRect, settings: &[u64], quads: &[(i8, usize, Vec<std::ops::Range<usize>>)], hashes: &[u64]) -> u64 {
        let mut k = Key::new(rect, settings);
        for (z, sub, ranges) in quads {
            k.quads(*z, *sub, ranges, hashes);
        }
        k.get()
    }

    const PANE: PixelRect = PixelRect { x: 74, y: 55, w: 721, h: 240 };

    #[test]
    fn the_same_quads_make_the_same_key() {
        let hashes = [11, 22, 33, 44, 55];
        let a = key(PANE, &[1, 2], &[(0, 1, vec![1..4])], &hashes);
        let b = key(PANE, &[1, 2], &[(0, 1, vec![1..4])], &hashes);
        assert_eq!(a, b);
    }

    #[test]
    fn the_same_quads_elsewhere_in_the_buffer_make_the_same_key() {
        // Another pane gaining a quad moves this one's quads along the buffer: what it draws
        // hasn't changed, so neither has its key
        let before = [7, 11, 22, 33];
        let after = [7, 8, 11, 22, 33];
        assert_eq!(
            key(PANE, &[], &[(0, 1, vec![1..4])], &before),
            key(PANE, &[], &[(0, 1, vec![2..5])], &after)
        );
        // ...and drawn in one range or two, the same quads in the same order
        assert_eq!(
            key(PANE, &[], &[(0, 1, vec![1..4])], &before),
            key(PANE, &[], &[(0, 1, vec![1..2, 2..4])], &before)
        );
    }

    #[test]
    fn any_quad_changing_changes_the_key() {
        let hashes = [11, 22, 33, 44];
        let base = key(PANE, &[], &[(0, 1, vec![0..4])], &hashes);
        for i in 0..4 {
            let mut changed = hashes;
            changed[i] ^= 1;
            assert_ne!(base, key(PANE, &[], &[(0, 1, vec![0..4])], &changed), "quad {i}");
        }
        // One quad more or fewer
        assert_ne!(base, key(PANE, &[], &[(0, 1, vec![0..3])], &hashes));
        // The same quads in another order
        assert_ne!(base, key(PANE, &[], &[(0, 1, vec![2..4, 0..2])], &hashes));
    }

    #[test]
    fn the_layer_the_size_the_place_and_the_settings_count() {
        let hashes = [11, 22];
        let base = key(PANE, &[5], &[(0, 1, vec![0..2])], &hashes);
        assert_ne!(base, key(PANE, &[5], &[(1, 1, vec![0..2])], &hashes), "another z-layer");
        assert_ne!(base, key(PANE, &[5], &[(0, 2, vec![0..2])], &hashes), "another buffer of it");
        assert_ne!(base, key(PixelRect { x: 75, ..PANE }, &[5], &[(0, 1, vec![0..2])], &hashes), "moved");
        assert_ne!(base, key(PixelRect { w: 722, ..PANE }, &[5], &[(0, 1, vec![0..2])], &hashes), "resized");
        assert_ne!(base, key(PANE, &[6], &[(0, 1, vec![0..2])], &hashes), "another atlas");
        assert_ne!(base, key(PANE, &[5, 0], &[(0, 1, vec![0..2])], &hashes), "a setting more");
        // Quads split between buffers differently aren't drawn in the same order
        assert_ne!(
            key(PANE, &[], &[(0, 0, vec![0..1]), (0, 1, vec![1..2])], &hashes),
            key(PANE, &[], &[(0, 1, vec![0..2])], &hashes)
        );
    }

    #[test]
    fn an_empty_visual_has_a_key_of_its_own() {
        // Cleared and nothing drawn: after that once, it's kept
        let empty = key(PANE, &[0], &[(0, 1, vec![]), (3, 0, vec![])], &[]);
        assert_eq!(empty, key(PANE, &[0], &[], &[]));
        assert_ne!(empty, key(PANE, &[1], &[], &[]), "cleared white, as the lights' swatch is");
    }

    #[test]
    fn only_quads_that_changed_are_written() {
        let held = [1, 2, 3, 4, 5, 6, 7, 8];
        // Nothing changed: nothing written
        assert!(dirty_runs(&held, &held, 4, 16).is_empty());
        // One quad, and another far from it: two runs
        let mut now = held;
        now[1] = 20;
        assert_eq!(dirty_runs(&now, &held, 1, 16), vec![1..2]);
        now[6] = 70;
        assert_eq!(dirty_runs(&now, &held, 2, 16), vec![1..2, 6..7]);
        // Within the gap of each other: one
        assert_eq!(dirty_runs(&now, &held, 5, 16), vec![1..7]);
        // Past the runs allowed: one from the first to the last
        assert_eq!(dirty_runs(&now, &held, 2, 1), vec![1..7]);
    }

    #[test]
    fn quads_past_what_was_written_are_written() {
        let held = [1, 2, 3];
        assert_eq!(dirty_runs(&[1, 2, 3, 4, 5], &held, 1, 16), vec![3..5]);
        // Never written: all of it
        assert_eq!(dirty_runs(&[1, 2], &[], 1, 16), vec![0..2]);
        // Fewer quads than were held: none of them changed, nothing to write
        assert!(dirty_runs(&[1, 2], &held, 1, 16).is_empty());
    }

    #[test]
    fn a_layer_grows_by_half_at_least() {
        // From nothing, what it needs, to the next 128
        assert_eq!(grow(0, 1), 128);
        assert_eq!(grow(0, 300), 384);
        // A little more than it held: half as many again
        assert_eq!(grow(1024, 1025), 1536);
        assert_eq!(grow(1000, 1001), 1536);
        // Far more: what it needs
        assert_eq!(grow(128, 5000), 5120);
    }

    #[test]
    fn a_quad_past_what_was_hashed_never_matches_another() {
        let hashes = [11, 22];
        assert_ne!(
            key(PANE, &[], &[(0, 1, vec![0..3])], &hashes),
            key(PANE, &[], &[(0, 1, vec![0..2])], &hashes)
        );
    }

    #[test]
    fn folding_one_word_differently_always_ends_apart() {
        // Each step is a bijection for a given word, so runs differing in one place differ
        for place in 0..8 {
            let run: Vec<u64> = (0..8).map(|i| i * 0x1234_5678 + 9).collect();
            let mut other = run.clone();
            other[place] = other[place].wrapping_add(1);
            let (mut a, mut b) = (0, 0);
            for (x, y) in run.iter().zip(&other) {
                fold(&mut a, *x);
                fold(&mut b, *y);
            }
            assert_ne!(a, b, "word {place}");
        }
        assert_ne!(pair(1.0, 0.0), pair(0.0, 1.0));
        assert_ne!(pair(0.0, 0.0), pair(-0.0, 0.0), "bit for bit");
    }

    #[test]
    fn floats_fold_bit_for_bit_two_at_a_time() {
        let fold_all = |values: &[f32]| {
            let mut hash = 0;
            fold_f32s(&mut hash, values);
            hash
        };
        let base: [f32; 5] = [1.0, 2.5, -3.0, 0.25, 7.0];
        for place in 0..base.len() {
            let mut other = base;
            other[place] = f32::from_bits(other[place].to_bits() ^ 1);
            assert_ne!(fold_all(&base), fold_all(&other), "value {place}, the odd last one included");
        }
        assert_ne!(fold_all(&[0.0, 1.0]), fold_all(&[-0.0, 1.0]), "bit for bit");
        assert_ne!(fold_all(&[1.0, 2.0]), fold_all(&[2.0, 1.0]), "in order");
        assert_ne!(fold_all(&[1.0, 2.0]), fold_all(&[1.0, 2.0, 0.0]), "one more, even a zero");
        // Pairs as fold and pair make them
        let mut by_hand = 0;
        fold(&mut by_hand, pair(1.0, 2.5));
        fold(&mut by_hand, pair(-3.0, 0.25));
        fold(&mut by_hand, 7.0f32.to_bits() as u64);
        assert_eq!(fold_all(&base), by_hand);
    }

    /// A stand-in for wezterm-gui's vertex: its place, and something that isn't
    type V = ([f32; 2], u32);

    fn down(v: &mut V, dy: f32) {
        v.0[1] += dy;
    }

    /// Three quads of four vertices laid out at row 0, the way a line's are, and their hashes
    fn line() -> (Vec<V>, Vec<u64>) {
        let mut vertices = vec![];
        for q in 0..3 {
            let x = q as f32 * 9.0;
            for (dx, y) in [(0.0, -540.0), (9.0, -540.0), (0.0, -520.0), (9.0, -520.0)] {
                vertices.push(([x + dx, y + q as f32 * 0.25], q));
            }
        }
        (vertices, vec![0x11, 0x22, 0x33])
    }

    fn placed(dy: f32) -> (Vec<V>, Vec<u64>) {
        let (from, hashes) = line();
        let mut into = vec![([0.0; 2], 0); from.len()];
        let mut into_hashes = vec![0; hashes.len()];
        assert_eq!(place(&mut into, &mut into_hashes, &from, &hashes, 4, dy, down), 3);
        (into, into_hashes)
    }

    #[test]
    fn a_line_placed_at_its_row_is_moved_down_by_it_and_nothing_else() {
        let (from, _) = line();
        // Row 7 of a pane 50 pixels down, rows 20 pixels high
        let dy = 50.0 + 7.0 * 20.0;
        let (into, _) = placed(dy);
        for (to, was) in into.iter().zip(&from) {
            assert_eq!(to.0[0], was.0[0], "across: as laid out");
            assert_eq!(to.0[1], was.0[1] + dy, "down: by its row");
            assert_eq!(to.1, was.1, "the rest as laid out");
        }
    }

    #[test]
    fn a_line_moved_a_row_changes_every_hash_and_the_same_row_keeps_them() {
        // The same line at the same row, frame after frame: the same hashes, so the GPU's copy
        // isn't written and the pane isn't drawn again for it
        assert_eq!(placed(190.0).1, placed(190.0).1);
        // Moved up a row, as a scroll moves it: every quad's hash changes, so the pane is drawn
        // again and its quads written, where the old hashes would leave the old frame up
        let (_, at) = placed(190.0);
        let (_, up) = placed(170.0);
        for (a, b) in at.iter().zip(&up) {
            assert_ne!(a, b);
        }
        // And across a screen of rows, no two quads of the line at any two rows hash alike
        let mut seen = std::collections::HashSet::new();
        for row in 0..200 {
            for hash in placed(50.0 + row as f32 * 20.0).1 {
                assert!(seen.insert(hash), "row {row}");
            }
        }
    }

    #[test]
    fn quads_that_differ_still_differ_moved() {
        for dy in [0.0f32, -0.0, 20.0, 190.0, 1e-3, -540.0] {
            let mut seen = std::collections::HashSet::new();
            for hash in 0..1000u64 {
                assert!(seen.insert(moved(hash.wrapping_mul(0x9e37_79b9_7f4a_7c15), dy)), "dy {dy}");
            }
        }
        // Moved by nothing still isn't the hash it was laid out with: every line's quads go
        // through `place`, so what counts is that all of them agree
        assert_ne!(moved(0x11, 0.0), 0x11);
        assert_ne!(moved(0x11, 0.0), moved(0x11, -0.0), "bit for bit");
    }

    #[test]
    fn a_line_is_placed_only_as_far_as_both_sides_hold() {
        let (from, hashes) = line();
        // Room for two quads: two written, the third left as it was
        let mut into = vec![([0.0; 2], 9); 8];
        let mut into_hashes = vec![7; 2];
        assert_eq!(place(&mut into, &mut into_hashes, &from, &hashes, 4, 20.0, down), 2);
        assert_eq!(into[7].0[1], from[7].0[1] + 20.0);
        // Fewer hashes than vertices: only the quads with one
        let mut into = vec![([0.0; 2], 9); 12];
        let mut into_hashes = vec![7; 3];
        assert_eq!(place(&mut into, &mut into_hashes, &from, &hashes[..1], 4, 20.0, down), 1);
        assert_eq!(into[4], ([0.0; 2], 9), "the second quad untouched");
        assert_eq!(into_hashes[1], 7);
    }

    #[test]
    fn a_swapchain_is_drawn_unless_it_shows_what_the_frame_would_draw() {
        assert!(must_draw(None, Some(1)), "just made or resized");
        assert!(must_draw(Some(1), Some(2)), "something in it changed");
        assert!(!must_draw(Some(2), Some(2)), "nothing in it changed");
        assert!(must_draw(Some(2), None), "drawn whatever it shows: a frame dump");
        assert!(must_draw(None, None));
    }

    #[test]
    fn a_uniform_is_known_by_what_the_shader_reads() {
        let projection = |x: f32| [[x, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let base = uniform_key([1.0, 1.0, 1.0], &projection(2.0));
        assert_eq!(base, uniform_key([1.0, 1.0, 1.0], &projection(2.0)));
        assert_ne!(base, uniform_key([1.0, 1.0, 1.0], &projection(2.5)), "another projection");
        assert_ne!(base, uniform_key([1.0, 1.0, 0.8], &projection(2.0)), "another colour transform");
        // Every one of the nineteen numbers counts, bit for bit
        let mut seen = std::collections::HashSet::new();
        seen.insert(base);
        for i in 0..19 {
            let mut hsb = [1.0f32, 1.0, 1.0];
            let mut p = projection(2.0);
            if i < 3 {
                hsb[i] = f32::from_bits(hsb[i].to_bits() ^ 1);
            } else {
                let v = &mut p[(i - 3) / 4][(i - 3) % 4];
                *v = f32::from_bits(v.to_bits() ^ 1);
            }
            assert!(seen.insert(uniform_key(hsb, &p)), "number {i}");
        }
    }

    #[test]
    fn uniforms_past_the_most_kept_go_oldest_first_down_to_half() {
        let kept = |used: &[u64], most: usize| {
            uniforms_kept_since(used.iter().copied(), most).map(|since| {
                let mut kept: Vec<u64> = used.iter().copied().filter(|&u| u >= since).collect();
                kept.sort();
                kept
            })
        };
        // At or under the most, none goes
        assert_eq!(kept(&[], 8), None);
        assert_eq!(kept(&[3, 1, 2, 7, 5, 4, 6, 0], 8), None);
        // Past it, the newest half stay, whatever order they're in
        assert_eq!(kept(&[3, 9, 1, 2, 8, 7, 5, 4, 6, 0], 8), Some(vec![6, 7, 8, 9]));
        assert_eq!(kept(&[9, 8, 7, 6, 5, 4, 3, 2, 1, 0], 8), Some(vec![6, 7, 8, 9]));
        // A frame draws many visuals, each its own uniform drawn in the same frame: all of the
        // frame the newest half reach into stay, never some of one frame's
        assert_eq!(kept(&[5, 5, 5, 5, 5, 1, 2, 3, 4], 4), Some(vec![5, 5, 5, 5, 5]));
        assert_eq!(kept(&[4, 4, 4, 1, 1, 1, 6, 6], 6), Some(vec![4, 4, 4, 6, 6]));
        // A window resized at every frame for longer than the most: each frame's few new ones
        // added, the kept ones never past the most and a frame's visuals more
        let (most, visuals) = (64usize, 7u64);
        let mut held: Vec<u64> = vec![];
        for frame in 0..1000u64 {
            held.extend(std::iter::repeat(frame).take(visuals as usize));
            if let Some(since) = uniforms_kept_since(held.iter().copied(), most) {
                held.retain(|&u| u >= since);
                assert!(held.len() >= most / 2, "frame {frame}: {}", held.len());
                assert!(held.contains(&frame), "frame {frame}: this frame's went");
            }
            assert!(held.len() <= most + visuals as usize, "frame {frame}: {}", held.len());
        }
        // Even a most of none keeps the newest
        assert_eq!(kept(&[2, 1], 0), Some(vec![2]));
    }

    #[test]
    fn uniforms_are_kept_for_the_size_drawn_at_and_the_one_before() {
        let hsb = [1.0, 1.0, 1.0];
        let size = |w: u32| uniforms_at(w, 600, hsb);
        let mut at = UniformsAt::default();
        assert_eq!(at.now(), None);
        // The first size and a second: nothing to let go yet
        assert_eq!(at.drawn_at(size(800)), None);
        assert_eq!(at.drawn_at(size(800)), None);
        assert_eq!(at.drawn_at(size(801)), None);
        assert_eq!(at.now(), Some(size(801)));
        // Back to the one before (a restore after a maximize): both kept, nothing goes
        assert_eq!(at.drawn_at(size(800)), None);
        assert_eq!(at.drawn_at(size(801)), None);
        // A third: the one before goes
        assert_eq!(at.drawn_at(size(802)), Some(size(800)));
        assert_eq!(at.now(), Some(size(802)));
        // A drag a pixel at a time: each size lets go of the one two back, so whatever the sizes,
        // the uniforms kept are two sizes' at most
        let mut held = vec![size(801), size(802)];
        for w in 803..2000 {
            if let Some(gone) = at.drawn_at(size(w)) {
                held.retain(|&h| h != gone);
            }
            held.push(size(w));
            assert_eq!(held, vec![size(w - 1), size(w)], "at {w}");
        }
        // The text's colour transform changed by a reload is a new one too, and changed back an
        // old one
        let mut at = UniformsAt::default();
        at.drawn_at(uniforms_at(800, 600, hsb));
        assert_eq!(at.drawn_at(uniforms_at(800, 600, [1.0, 1.0, 0.9])), None);
        assert_eq!(at.drawn_at(uniforms_at(800, 600, hsb)), None);
        assert_eq!(at.drawn_at(uniforms_at(800, 601, hsb)), Some(uniforms_at(800, 600, [1.0, 1.0, 0.9])));
        // Width and height aren't interchangeable
        assert_ne!(uniforms_at(800, 600, hsb), uniforms_at(600, 800, hsb));
    }

    #[test]
    fn motion_settings_are_made_again_only_for_what_they_read() {
        // 'on' and 'off' read nothing of Windows': its changes don't make them again
        assert_eq!(settings_seen("on", 7), 0);
        assert_eq!(settings_seen("off", 7), 0);
        assert!(motion_holds(true, settings_seen("on", 0), settings_seen("on", 7)));
        // 'system' reads Animation effects: each change Windows says of may change them
        assert_eq!(settings_seen("system", 7), 7);
        assert!(motion_holds(true, settings_seen("system", 3), settings_seen("system", 3)));
        assert!(!motion_holds(true, settings_seen("system", 3), settings_seen("system", 4)));
        // Another config (a reload, the window's own overrides) makes them again whatever else
        assert!(!motion_holds(false, 0, 0));
    }

    #[test]
    fn a_frames_work_adds_up() {
        let mut work = Work::default();
        work.made(2);
        work.uploaded(272);
        work.uploaded(544);
        let us = std::time::Duration::from_micros;
        work.recorded(3, 1, [us(40), us(120), us(300)]);
        work.recorded(1, 1, [us(1), us(2), us(3)]);
        assert_eq!(
            work,
            Work { made: 2, uploads: 2, upload_bytes: 816, passes: 4, submits: 2, upload_us: 41, record_us: 122, submit_us: 303, stale: 0 }
        );
    }

    #[test]
    fn a_quad_whose_hash_matches_but_whose_vertices_dont_is_stale() {
        // Four vertices a quad, three quads
        let held_v = [1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3];
        let held = [10, 20, 30];
        // The same: nothing stale
        assert_eq!(stale_quads(&held, &held, &held_v, &held_v, 4, 3), 0);
        // The middle quad written with its hash changed: written, so not stale
        let now_v = [1, 1, 1, 1, 9, 2, 2, 2, 3, 3, 3, 3];
        assert_eq!(stale_quads(&[10, 21, 30], &held, &now_v, &held_v, 4, 3), 0);
        // The same write with the hash left as it was: stale
        assert_eq!(stale_quads(&held, &held, &now_v, &held_v, 4, 3), 1);
        // Only the quads asked about, and none past what was held
        assert_eq!(stale_quads(&held, &held, &now_v, &held_v, 4, 1), 0);
        assert_eq!(stale_quads(&[10, 20, 30, 40], &held, &[now_v.as_slice(), &[4, 4, 4, 4]].concat(), &held_v, 4, 4), 1);
    }
}
