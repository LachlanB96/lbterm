//! LBTerm: what font setup costs, counted, and what fonts share so it's done once (lbterm/PERF.md,
//! "What font setup costs").
//!
//! - [`Tally`]s of each kind of font work, always on (a relaxed atomic add or two each), which the
//!   control pipe's `metrics` reports under `fonts` (wezterm-gui's counters.rs), so a test can say
//!   a config save made no font, read no font file and searched for no fallback.
//! - [`Found`]: the fallback faces the searches of every font with the same fallback settings
//!   and the same files in its font dirs have found, by character. Upstream kept them per font
//!   (`LoadedFont`), so the pane's regular font, its bold, the tab bar's font, the rail's labels and
//!   each of the Conductor's sizes searched for the same character on their own, each find
//!   reshaping the whole window, and a reload or a zoom, which makes the fonts again, lost them all
//!   and searched again. A font takes from it only the faces of characters it meets, never every
//!   face found (PITFALLS 54).
//! - [`built_in_database`]: the bundled fonts, parsed once a process rather than once a window
//!   (and once more for each throwaway font configuration a new window's size is worked out with).

use crate::db::FontDatabase;
use crate::parser::ParsedFont;
use rangeset::RangeSet;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

/// How often something ran and how long it took in all
pub struct Tally {
    count: AtomicU64,
    ns: AtomicU64,
}

impl Tally {
    pub const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            ns: AtomicU64::new(0),
        }
    }

    /// `n` more, taking `ns` in all
    pub fn add(&self, n: u64, ns: u64) {
        self.count.fetch_add(n, Relaxed);
        if ns > 0 {
            self.ns.fetch_add(ns, Relaxed);
        }
    }

    /// One more, untimed
    pub fn one(&self) {
        self.add(1, 0)
    }

    /// One more, that started at `start`
    pub fn since(&self, start: Instant) {
        self.add(1, start.elapsed().as_nanos() as u64)
    }

    /// (how many, nanoseconds in all), zeroed with `reset`
    pub fn read(&self, reset: bool) -> (u64, u64) {
        if reset {
            (self.count.swap(0, Relaxed), self.ns.swap(0, Relaxed))
        } else {
            (self.count.load(Relaxed), self.ns.load(Relaxed))
        }
    }
}

/// Font configurations made: a window's, and the throwaways a new window's size is worked out with
pub static CONFIGURATIONS: Tally = Tally::new();
/// Fonts made (`LoadedFont`): a style resolved through the font lookup, its shaper and metrics
pub static FONTS: Tally = Tally::new();
/// Lookups through the system's font locator (`load_fonts`), one a group of a style's families
pub static LOOKUPS: Tally = Tally::new();
/// Font files GDI's lookup copied out and parsed (the whole file, a substitute's for a family
/// Windows doesn't have, until LBTerm asked GDI the face's name first)
pub static FILES_READ: Tally = Tally::new();
/// Families GDI put another face in place of, turned down by name before reading its file
pub static SUBSTITUTES: Tally = Tally::new();
/// The bundled fonts parsed (once a process since LBTerm keeps them)
pub static BUILT_IN: Tally = Tally::new();
/// Searches the fallback thread made for characters no face of a font has, by the system's
/// fallback, the font dirs and the bundled fonts
pub static FALLBACK_SEARCHES: Tally = Tally::new();
/// Characters a font needed a fallback for that another font with the same settings had searched
/// for already: answered from what it found
pub static FALLBACK_SHARED: Tally = Tally::new();
/// Shapers built: one a font, and again each time its fallbacks grow
pub static SHAPERS: Tally = Tally::new();
/// Reloads whose font settings were the same as before, so the fonts were kept
pub static KEPT: Tally = Tally::new();

/// Every tally by name, as `metrics` reports them: (name, how many, nanoseconds)
pub fn report(reset: bool) -> Vec<(&'static str, u64, u64)> {
    [
        ("configurations", &CONFIGURATIONS),
        ("fonts", &FONTS),
        ("lookups", &LOOKUPS),
        ("files_read", &FILES_READ),
        ("substitutes", &SUBSTITUTES),
        ("built_in", &BUILT_IN),
        ("fallback_searches", &FALLBACK_SEARCHES),
        ("fallback_shared", &FALLBACK_SHARED),
        ("shapers", &SHAPERS),
        ("kept", &KEPT),
    ]
    .into_iter()
    .map(|(name, t)| {
        let (n, ns) = t.read(reset);
        (name, n, ns)
    })
    .collect()
}

/// What fallback searches found, for every font with the same fallback settings and font dirs:
/// each face found, in the order found, and for each character searched for the faces that cover
/// it (none when no face does)
#[derive(Default)]
pub struct Found {
    faces: Vec<ParsedFont>,
    chars: HashMap<char, Vec<usize>>,
}

/// One [`Found`], shared by the fonts that use it and the fallback thread
pub type SharedFound = Arc<Mutex<Found>>;

impl Found {
    /// Every face found so far, in the order found
    pub fn faces(&self) -> &[ParsedFont] {
        &self.faces
    }

    /// Whether `c` has been searched for, found or not
    pub fn searched(&self, c: char) -> bool {
        self.chars.contains_key(&c)
    }

    /// Of `wanted`: the faces found for those already searched for (each once, in the order
    /// found), how many of them were, and the characters still to search for
    pub fn split(&self, wanted: &[char]) -> (Vec<ParsedFont>, usize, Vec<char>) {
        let mut idx = vec![];
        let mut known = 0;
        let mut rest = vec![];
        for c in wanted {
            match self.chars.get(c) {
                Some(faces) => {
                    known += 1;
                    idx.extend(faces.iter().copied());
                }
                None => rest.push(*c),
            }
        }
        idx.sort_unstable();
        idx.dedup();
        (idx.into_iter().map(|i| self.faces[i].clone()).collect(), known, rest)
    }

    /// Forgets the characters searched for that no face covered, so the next font to meet one
    /// searches for it again; how many. A reload that makes the fonts again does it: upstream
    /// searched again with fresh fonts after every such reload, and a font installed in Windows
    /// since may have the character (DirectWrite's system collection sees it through the font
    /// cache service). What was found stays, so nothing found is searched for twice
    pub fn forget_missing(&mut self) -> usize {
        let before = self.chars.len();
        self.chars.retain(|_, faces| !faces.is_empty());
        before - self.chars.len()
    }

    /// What a search for `searched` found: each face with the characters of `searched` it covers.
    /// A character no face covers is noted as searched with none, so nothing searches for it again
    /// until a reload makes the fonts again ([`Found::forget_missing`]).
    pub fn add(&mut self, searched: &[char], found: Vec<(ParsedFont, RangeSet<u32>)>) {
        for c in searched {
            self.chars.entry(*c).or_default();
        }
        for (face, covers) in found {
            let i = match self.faces.iter().position(|f| *f == face) {
                Some(i) => i,
                None => {
                    self.faces.push(face);
                    self.faces.len() - 1
                }
            };
            for c in covers.iter_values().filter_map(char::from_u32) {
                let faces = self.chars.entry(c).or_default();
                if !faces.contains(&i) {
                    faces.push(i);
                }
            }
        }
    }
}

/// What the font dirs hold, as a fallback search sees them: each file under them, with its length
/// and when it was last written, walked as `FontDatabase::with_font_dirs` walks them. Empty
/// without font dirs (Lachlan's config has none), so it costs nothing there. A file added, removed
/// or written changes it, and with it what a search can find: upstream read the font dirs again on
/// every reload and for every window, so a font file added there was used after the next one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FontDirs(Vec<(PathBuf, u64, Option<SystemTime>)>);

impl FontDirs {
    /// The font dirs of `config`, walked now
    pub fn of(config: &config::Config) -> Self {
        let mut files = vec![];
        for path in &config.font_dirs {
            for entry in walkdir::WalkDir::new(path).into_iter().filter_map(Result::ok) {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        files.push((entry.path().to_path_buf(), meta.len(), meta.modified().ok()));
                    }
                }
            }
        }
        Self(files)
    }
}

/// The [`Found`] of the fallback settings `key` ([`config::lbterm::fallback_key`]) and the font
/// dirs as they are (`dirs`): every font configuration with the same settings and files shares
/// one, across a zoom, a font size changed and the process's windows. A search finds the same
/// faces for the same characters whatever font asked (it asks for them at 16 px), and a font
/// that has the character never looks. A file changed in the font dirs makes a new one, so what
/// a search found missing, or found elsewhere, before it came is searched for again.
pub fn found_for(key: &config::lbterm::FontKey, dirs: &FontDirs) -> SharedFound {
    static ALL: Mutex<Vec<(config::lbterm::FontKey, FontDirs, SharedFound)>> =
        Mutex::new(Vec::new());
    let mut all = ALL.lock().unwrap_or_else(|e| e.into_inner());
    // Settings no font configuration uses any more go: only ALL holds their Found
    all.retain(|(k, d, found)| (k == key && d == dirs) || Arc::strong_count(found) > 1);
    if let Some((_, _, found)) = all.iter().find(|(k, d, _)| k == key && d == dirs) {
        return Arc::clone(found);
    }
    let found = SharedFound::default();
    all.push((key.clone(), dirs.clone(), Arc::clone(&found)));
    found
}

/// The bundled fonts' database, parsed the first time it's asked for and shared after: 30 faces
/// (JetBrains Mono's 16, Roboto's 12, Noto Color Emoji and Symbols Nerd Font Mono), which upstream
/// parsed for every font configuration, a window's and each throwaway one's
pub fn built_in_database() -> anyhow::Result<Arc<FontDatabase>> {
    static DB: Mutex<Option<Arc<FontDatabase>>> = Mutex::new(None);
    let mut db = DB.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(db) = db.as_ref() {
        return Ok(Arc::clone(db));
    }
    let start = Instant::now();
    let made = Arc::new(FontDatabase::with_built_in()?);
    BUILT_IN.since(start);
    *db = Some(Arc::clone(&made));
    Ok(made)
}

#[cfg(test)]
mod test {
    use super::*;

    fn built_in(family: &str) -> ParsedFont {
        let db = built_in_database().unwrap();
        db.candidates(&config::FontAttributes::new(family))
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("{family} is bundled"))
            .clone()
    }

    fn set(chars: &[char]) -> RangeSet<u32> {
        let mut s = RangeSet::new();
        for c in chars {
            s.add(*c as u32);
        }
        s
    }

    #[test]
    fn the_built_in_fonts_are_parsed_once() {
        let a = built_in_database().unwrap();
        let (before, _) = BUILT_IN.read(false);
        let b = built_in_database().unwrap();
        assert!(Arc::ptr_eq(&a, &b), "the same database");
        assert_eq!(BUILT_IN.read(false).0, before, "not parsed again");
        assert!(!a.list_available().is_empty());
    }

    #[test]
    fn what_a_search_found_answers_the_next_font() {
        let emoji = built_in("Noto Color Emoji");
        let symbols = built_in("Symbols Nerd Font Mono");
        let mut found = Found::default();
        let (faces, known, rest) = found.split(&['a', 'b']);
        assert!(faces.is_empty());
        assert_eq!(known, 0);
        assert_eq!(rest, vec!['a', 'b'], "nothing searched yet");

        // A search for a, b and c: a in the emoji face, b in both, c in none
        found.add(
            &['a', 'b', 'c'],
            vec![(emoji.clone(), set(&['a', 'b'])), (symbols.clone(), set(&['b']))],
        );
        assert!(found.searched('c'), "found in none is still searched");
        assert!(!found.searched('d'));
        let (faces, known, rest) = found.split(&['b', 'c', 'd']);
        assert_eq!(known, 2);
        assert_eq!(rest, vec!['d'], "only d is still to search for");
        assert_eq!(faces, vec![emoji.clone(), symbols.clone()], "b's faces, each once, in the order found");
        let (faces, _, _) = found.split(&['c']);
        assert!(faces.is_empty(), "c has no face");

        // A face found again isn't kept twice
        found.add(&['e'], vec![(symbols.clone(), set(&['e']))]);
        assert_eq!(found.faces().len(), 2);
        let (faces, _, _) = found.split(&['e']);
        assert_eq!(faces, vec![symbols.clone()]);

        // A reload that makes the fonts again forgets what nothing had, and only that
        assert_eq!(found.forget_missing(), 1);
        assert!(!found.searched('c'), "searched for again");
        assert!(found.searched('a') && found.searched('b') && found.searched('e'));
        let (faces, known, rest) = found.split(&['b', 'c']);
        assert_eq!((known, rest), (1, vec!['c']));
        assert_eq!(faces, vec![emoji, symbols]);
        assert_eq!(found.forget_missing(), 0);
    }

    #[test]
    fn settings_alike_share_what_was_found() {
        let mut config = config::Config::default_config();
        config.font_dirs = vec!["fonts-a".into()];
        let a = config::lbterm::fallback_key(&config);
        config.font_size = 32.0;
        assert!(a == config::lbterm::fallback_key(&config), "a size is no part of it");
        config.font_dirs = vec!["fonts-b".into()];
        let b = config::lbterm::fallback_key(&config);
        let none = FontDirs::default();
        let one = found_for(&a, &none);
        let two = found_for(&a, &none);
        assert!(Arc::ptr_eq(&one, &two), "the same settings share one");
        let other = found_for(&b, &none);
        assert!(!Arc::ptr_eq(&one, &other), "other settings have their own");
        one.lock().unwrap().add(&['x'], vec![]);
        assert!(two.lock().unwrap().searched('x'));
        assert!(!other.lock().unwrap().searched('x'));
        let changed = FontDirs(vec![("fonts-a/new.ttf".into(), 1, None)]);
        let fresh = found_for(&a, &changed);
        assert!(!Arc::ptr_eq(&one, &fresh), "a file in the font dirs changed: searched again");
        assert!(!fresh.lock().unwrap().searched('x'));
    }

    #[test]
    fn the_font_dirs_are_listed_file_by_file() {
        let dir = std::env::temp_dir().join(format!("lbterm-fontdirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let mut config = config::Config::default_config();
        assert_eq!(FontDirs::of(&config), FontDirs::default(), "no font dirs, nothing walked");
        config.font_dirs = vec![dir.clone()];
        let empty = FontDirs::of(&config);
        assert!(empty.0.is_empty(), "folders aren't files");
        std::fs::write(dir.join("sub").join("a.ttf"), b"not a font").unwrap();
        let one = FontDirs::of(&config);
        assert_eq!(one.0.len(), 1, "a file in a folder under it");
        assert_ne!(one, empty);
        assert_eq!(FontDirs::of(&config), one, "the same files, the same listing");
        std::fs::write(dir.join("sub").join("a.ttf"), b"a longer file, not a font").unwrap();
        assert_ne!(FontDirs::of(&config), one, "a file written again");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
