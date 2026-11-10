//! What changes in a set of folders and files, from Windows' own word (ReadDirectoryChangesW,
//! through notify), so a thread that shows what they hold sleeps until one changes and looks at
//! once when it does, instead of looking every so often: the InMail folders (inbox.rs), the
//! Conductor's state file and the tab links' file, the images thumbnails are drawn of, the
//! version.txt the restart pill follows, the subagents' transcripts the tasks card reads, and
//! the transcript the scroll strip shows (conversation.rs).
//!
//! Windows watches folders, so a [`Target`] is watched through one: a folder itself, whose every
//! file and folder matters, or a file's folder, where only the file does. One that isn't there yet
//! is watched from the nearest folder above it that is, which sees it made, and [`Watch::follow`]
//! moves down to it once it's there. A folder sees every change in it, so a file whose folder is
//! busy (the machine state's, where a dozen programs write) wakes notify's thread for each of its
//! neighbours' writes too; those stop there, and only a change that can be about a target is
//! passed on to whoever watches ([`Watch::matters`]). A file whose folder isn't known, only that
//! it's in one of a folder's folders (a Claude Code session's transcript not written yet, in the
//! project folder of wherever it started), is [`Target::Within`], watched through that folder and
//! every folder in it.
//!
//! notify's own thread used to wake every 100 ms whatever happened (deps/notify/LBTERM.md); it
//! sleeps until a change comes now.
//!
//! A watched folder can go, or move, under its watch: removed (Windows ends its watch, which
//! notify passes on as an error), or renamed (its watch follows it to its new name, and Windows
//! says nothing to it; notify watches each watched folder's own name in its parent for that,
//! folders' names alone). Either way [`Watch::follow`] watches it again, or the nearest folder
//! above it that's there, and whoever follows looks at what it watches again. A change Windows
//! couldn't keep (more at once than the watch holds) is an error too, and whoever follows looks.
//! Only a folder further up than its parent renamed goes unseen, until the next follow
//! (lbterm/AT-REST.md).
//!
//! A watch holds a handle open on the folder it watches and, for its name, on that folder's
//! parent, and Windows won't rename or remove a folder while a handle inside it is open. So a
//! program about to rename a folder asks every watch in the process to let go of it ([`let_go`],
//! from crate::let_go's requests): while it's let go of, a watch whose folder is that folder or in
//! it watches the nearest folder above it instead, which holds nothing inside it and still sees it
//! renamed or removed, and once it's taken back every such watch follows again
//! (lbterm/LET-GO.md).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

/// What a [`Watch`] saw
#[derive(Debug)]
pub enum Seen {
    /// Something changed at these paths
    Changed(Vec<PathBuf>),
    /// The watch went wrong (more changed at once than it could hold, a folder went with its
    /// watch): what it missed can only be found by looking, after a [`Watch::follow`], which
    /// watches again what went
    Failed(String),
}

/// What's watched for
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A folder: a file or folder in it changing matters, and so does a folder on the way to it
    Folder(PathBuf),
    /// A file: only it matters, and a folder on the way to it
    File(PathBuf),
    /// A file of this name in any folder right inside this one: watched through it and all its
    /// folders, so it's meant for a file not there yet whose folder can't be known
    Within(PathBuf, String),
}

impl Target {
    pub fn path(&self) -> &Path {
        match self {
            Target::Folder(p) | Target::File(p) | Target::Within(p, _) => p,
        }
    }

    /// The folder Windows is asked to watch for it: a folder itself, a file's folder, or while
    /// that isn't there, the nearest folder above it that is
    pub fn folder(&self) -> Option<PathBuf> {
        let from = match self {
            Target::Folder(p) | Target::Within(p, _) => p.as_path(),
            Target::File(p) => p.parent()?,
        };
        from.ancestors()
            .find(|p| !p.as_os_str().is_empty() && p.is_dir())
            .map(Path::to_path_buf)
    }

    /// Whether its folder is watched with all the folders in it: a [`Target::Within`] once its
    /// folder is there (until then the folder above, which sees it made, is watched alone)
    pub fn deep(&self) -> bool {
        matches!(self, Target::Within(p, _) if p.is_dir())
    }

    /// Whether a change at `path` can change what it holds: the file itself, or for a folder a
    /// file or folder in it; or a folder on the way to either (made, removed or renamed)
    pub fn matters(&self, path: &Path) -> bool {
        self.holds(path) || self.on_the_way(path)
    }

    /// Whether a change of `kind` at `path` can change what it holds. A folder on the way only
    /// matters made, removed or renamed: a file changing in it changes the folder's own time,
    /// which its parent's watch says too, and its own watch said already, so each write beside a
    /// watched file came twice (a busy folder of the machine state's under the folder of another
    /// file watched woke the reading thread every second or so for nothing)
    pub fn matters_as(&self, path: &Path, kind: &notify::EventKind) -> bool {
        use notify::event::{EventKind, ModifyKind};
        self.holds(path)
            || (self.on_the_way(path)
                && !matches!(
                    kind,
                    EventKind::Modify(ModifyKind::Any | ModifyKind::Data(_) | ModifyKind::Metadata(_)) | EventKind::Access(_)
                ))
    }

    /// The file itself, or for a folder the folder or a file or folder in it; for a file within a
    /// folder, the folder (made, it's watched with all its folders from then) or a file of its
    /// name in a folder in it
    fn holds(&self, path: &Path) -> bool {
        match self {
            Target::Folder(folder) => {
                same_path(folder, path) || path.parent().map_or(false, |parent| same_path(parent, folder))
            }
            Target::File(file) => same_path(file, path),
            Target::Within(folder, name) => {
                same_path(folder, path)
                    || (path.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
                        && path.parent().and_then(Path::parent).is_some_and(|f| same_path(f, folder)))
            }
        }
    }

    /// A folder above it
    fn on_the_way(&self, path: &Path) -> bool {
        starts_with(self.path(), path) && !same_path(self.path(), path)
    }
}

/// The folders to watch for `targets` ([`Target::folder`]), each once
pub fn folders(targets: &[Target]) -> Vec<PathBuf> {
    deep_folders(targets).into_iter().map(|(f, _)| f).collect()
}

/// The folders to watch for `targets`, each once, and whether with the folders in it
/// ([`Target::deep`]): one wanted both ways is watched deep. A folder that is, or is in, a folder
/// let go of ([`let_go`]) gives way to the nearest folder above that one, watched alone.
fn deep_folders(targets: &[Target]) -> Vec<(PathBuf, bool)> {
    let let_go = let_go_of();
    let mut folders: Vec<(PathBuf, bool)> = vec![];
    for t in targets {
        let Some(there) = t.folder() else {
            continue;
        };
        let Some((there, moved)) = outside(there, &let_go) else {
            continue;
        };
        let deep = !moved && t.deep();
        match folders.iter_mut().find(|(f, _)| same_path(f, &there)) {
            Some((_, d)) => *d |= deep,
            None => folders.push((there, deep)),
        }
    }
    folders
}

/// Two paths are the same: compared part by part, letters in any case, as Windows does
pub fn same_path(a: &Path, b: &Path) -> bool {
    a.components().count() == b.components().count() && starts_with(b, a)
}

/// `path` is `prefix` or inside it, compared part by part, letters in any case
pub fn starts_with(path: &Path, prefix: &Path) -> bool {
    let mut parts = path.components();
    prefix.components().all(|want| {
        parts.next().map_or(false, |got| {
            got.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&want.as_os_str().to_string_lossy())
        })
    })
}

/// The folders every watch in the process has let go of ([`let_go`])
static LET_GO: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Every watch made, by what its notify thread knows and how it tells whoever follows it, so a
/// folder let go of or taken back reaches the watches it's about at once. Weak: a watch dropped
/// drops out
static WATCHES: Mutex<Vec<(Weak<Mutex<Shared>>, Weak<Mutex<Teller>>)>> = Mutex::new(Vec::new());

/// How a watch tells whoever follows it what it saw: from notify's thread, and from [`let_go`]
type Teller = Box<dyn FnMut(Seen) + Send>;

fn locked<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The folders let go of now
pub fn let_go_of() -> Vec<PathBuf> {
    locked(&LET_GO).clone()
}

/// The folder to watch in place of `folder`, and whether it moved: itself, or while it is or is
/// in a folder let go of, the nearest folder that's there above that one (and above any other
/// let go of that it is or is in). Nothing when no folder above is there.
fn outside(folder: PathBuf, let_go: &[PathBuf]) -> Option<(PathBuf, bool)> {
    let mut folder = folder;
    let mut moved = false;
    while let Some(gone) = let_go.iter().find(|g| starts_with(&folder, g)) {
        // Each step goes up past the folder let go of, so it ends
        folder = gone
            .parent()?
            .ancestors()
            .find(|p| !p.as_os_str().is_empty() && p.is_dir())?
            .to_path_buf();
        moved = true;
    }
    Some((folder, moved))
}

/// Has every watch in the process let go of `folders` and of no others, so a program can rename
/// or remove them (crate::let_go, lbterm/LET-GO.md): a watch whose folder is one of them or in one
/// watches the nearest folder above it instead, which sees it renamed or removed and holds no
/// handle inside it, and a folder taken back is watched again. Each watch with a target a change
/// in the set can be about is told it, as [`Seen::Changed`] with the folders let go of or taken
/// back, so whoever follows it follows again (and looks, as for a folder made or removed): its
/// handles go then, on its own thread. Says whether the set changed.
pub fn let_go(folders: Vec<PathBuf>) -> bool {
    let changed: Vec<PathBuf> = {
        let mut now = locked(&LET_GO);
        let mut want: Vec<PathBuf> = vec![];
        for f in folders {
            if !want.iter().any(|w| same_path(w, &f)) {
                want.push(f);
            }
        }
        let taken_back = now.iter().filter(|n| !want.iter().any(|w| same_path(w, n)));
        let new = want.iter().filter(|w| !now.iter().any(|n| same_path(n, w)));
        let changed: Vec<PathBuf> = taken_back.chain(new).cloned().collect();
        *now = want;
        changed
    };
    if changed.is_empty() {
        return false;
    }
    let watches: Vec<(Arc<Mutex<Shared>>, Arc<Mutex<Teller>>)> = {
        let mut all = locked(&WATCHES);
        all.retain(|(shared, tell)| shared.strong_count() > 0 && tell.strong_count() > 0);
        all.iter()
            .filter_map(|(shared, tell)| Some((shared.upgrade()?, tell.upgrade()?)))
            .collect()
    };
    for (shared, tell) in watches {
        // Only a watch it's about: the rest sleep on
        if matter(&lock(&shared).targets, &changed) {
            let mut tell = locked(&tell);
            (*tell)(Seen::Changed(changed.clone()));
        }
    }
    true
}

/// A set of [`Target`]s watched through their folders, telling a callback, from notify's thread,
/// of every change that can be about one of them and of its failures.
pub struct Watch {
    watcher: notify::RecommendedWatcher,
    /// The folders watched now, and those of them watched with their folders
    watched: Vec<PathBuf>,
    deep: Vec<PathBuf>,
    /// What notify's thread knows, which passes on only what matters
    shared: Arc<Mutex<Shared>>,
    /// The callback, held here so [`let_go`] can reach it while the watch lives
    _tell: Arc<Mutex<Teller>>,
}

/// What a [`Watch`] shares with notify's thread
#[derive(Default)]
struct Shared {
    /// What's watched for
    targets: Vec<Target>,
    /// The folders watched now
    watched: Vec<PathBuf>,
    /// Folders watched whose watch went with them (removed, renamed, failed): watched again at
    /// the next follow
    gone: Vec<PathBuf>,
}

fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// A change of `kind` to a folder itself that its own watch doesn't follow: made, removed or
/// renamed
fn moves_a_folder(kind: &notify::EventKind) -> bool {
    use notify::event::{EventKind, ModifyKind};
    matches!(kind, EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_)))
}

/// Whether a change at any of `paths` can change one of `targets`
fn matter(targets: &[Target], paths: &[PathBuf]) -> bool {
    paths.iter().any(|path| targets.iter().any(|t| t.matters(path)))
}

impl Watch {
    /// A watch telling `seen` of every change that can be about what it watches, and of its
    /// failures, and of a folder on the way to a target let go of or taken back ([`let_go`]). It
    /// watches nothing until [`Self::follow`].
    pub fn new(seen: impl FnMut(Seen) + Send + 'static) -> notify::Result<Self> {
        let shared: Arc<Mutex<Shared>> = Arc::default();
        let theirs = Arc::clone(&shared);
        let tell: Arc<Mutex<Teller>> = Arc::new(Mutex::new(Box::new(seen)));
        let told = Arc::clone(&tell);
        let seen = move |s: Seen| {
            let mut tell = locked(&told);
            (*tell)(s)
        };
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
            Ok(event) => {
                let matters = {
                    let mut shared = lock(&theirs);
                    // A folder watched made, removed or renamed (its name's watch, or its
                    // parent's, says so): its watch went with it, or follows it to its new name
                    if moves_a_folder(&event.kind) {
                        let went: Vec<PathBuf> = event
                            .paths
                            .iter()
                            .filter(|p| shared.watched.iter().any(|w| same_path(w, p)))
                            .cloned()
                            .collect();
                        shared.gone.extend(went);
                    }
                    shared
                        .targets
                        .iter()
                        .any(|t| event.paths.iter().any(|p| t.matters_as(p, &event.kind)))
                };
                // A neighbour's change stops here, on notify's thread, waking nobody else
                if matters {
                    seen(Seen::Changed(event.paths))
                }
            }
            // Its folder went (removed, or can't be read), or Windows couldn't keep what changed:
            // what it was about is watched again, and whoever follows looks
            Err(err) => {
                lock(&theirs).gone.extend(err.paths.iter().cloned());
                seen(Seen::Failed(format!("{err:#}")))
            }
        })?;
        {
            let mut all = locked(&WATCHES);
            all.retain(|(shared, tell)| shared.strong_count() > 0 && tell.strong_count() > 0);
            all.push((Arc::downgrade(&shared), Arc::downgrade(&tell)));
        }
        Ok(Self {
            watcher,
            watched: vec![],
            deep: vec![],
            shared,
            _tell: tell,
        })
    }

    /// Watches the folders `targets` needs now ([`folders`]) and no others, and says whether that
    /// changed what's watched: then a folder may have come with another under it already, or the
    /// file with it, so whoever follows asks again and then looks. A folder whose watch went with
    /// it (removed, renamed, failed) is let go and watched again, or the nearest folder above it
    /// that's there. A folder that's there and can't be watched is said in the log and tried
    /// again at the next follow; one that went before it could be watched is a change too, so the
    /// next follow watches above it.
    pub fn follow(&mut self, targets: &[Target]) -> bool {
        use notify::Watcher;
        // What matters first, so a change in a folder about to be watched is passed on
        let gone = {
            let mut shared = lock(&self.shared);
            shared.targets = targets.to_vec();
            std::mem::take(&mut shared.gone)
        };
        let want = deep_folders(targets);
        let mut changed = false;
        let mut kept = vec![];
        for path in std::mem::take(&mut self.watched) {
            let deep = self.deep.iter().any(|d| same_path(d, &path));
            let went = gone.iter().any(|g| same_path(g, &path));
            if !went && want.iter().any(|(w, d)| same_path(w, &path) && *d == deep) {
                kept.push(path);
            } else {
                // Its watch ended, or follows the folder to another name: it's let go of, and
                // watched again below if it's still wanted
                let _ = self.watcher.unwatch(&path);
                self.deep.retain(|d| !same_path(d, &path));
                changed = true;
            }
        }
        self.watched = kept;
        for (folder, deep) in want {
            if self.watched.iter().any(|w| same_path(w, &folder)) {
                continue;
            }
            let mode = if deep { notify::RecursiveMode::Recursive } else { notify::RecursiveMode::NonRecursive };
            match self.watcher.watch(&folder, mode) {
                Ok(()) => {
                    if deep {
                        self.deep.push(folder.clone());
                    }
                    self.watched.push(folder);
                    changed = true;
                }
                // It went between being looked for and being watched
                Err(_) if !folder.is_dir() => changed = true,
                Err(err) => log::error!(
                    "watch: can't watch {}, so a change there shows only once something else is seen: {err:#}",
                    folder.display()
                ),
            }
        }
        lock(&self.shared).watched = self.watched.clone();
        changed
    }

    /// The folders watched now
    pub fn watched(&self) -> &[PathBuf] {
        &self.watched
    }

    /// Whether a change at any of `paths` can change one of the targets ([`Target::matters`]):
    /// one a follow since took out doesn't
    pub fn matters(&self, paths: &[PathBuf]) -> bool {
        matter(&lock(&self.shared).targets, paths)
    }

    /// The targets a change at any of `paths` can change
    pub fn which(&self, paths: &[PathBuf]) -> Vec<Target> {
        lock(&self.shared)
            .targets
            .iter()
            .filter(|t| paths.iter().any(|p| t.matters(p)))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::mpsc::{channel, Receiver};
    use std::time::{Duration, Instant};

    #[test]
    fn a_file_is_watched_through_its_folder_and_a_missing_one_from_above() {
        let root = tempfile::tempdir().unwrap();
        let here = root.path().join("conductor");
        std::fs::create_dir_all(&here).unwrap();
        let targets = [
            Target::File(here.join("button.json")),
            Target::File(here.join("links.json")),
            Target::File(root.path().join("later").join("deeper").join("version.txt")),
            Target::Folder(root.path().join("InMail")),
        ];
        assert_eq!(
            folders(&targets),
            vec![here.clone(), root.path().to_path_buf()],
            "the two files' folder once, and the root once for the two that aren't there yet"
        );
        let shouty = [
            Target::File(PathBuf::from(here.to_string_lossy().to_uppercase()).join("A")),
            Target::File(here.join("B")),
        ];
        assert_eq!(folders(&shouty).len(), 1, "named in another case, it's the same folder");
    }

    #[test]
    fn a_file_matters_alone_and_a_folder_with_what_is_in_it() {
        let root = tempfile::tempdir().unwrap();
        let file = Target::File(root.path().join("state").join("button.json"));
        assert!(file.matters(&root.path().join("state").join("button.json")));
        assert!(file.matters(&root.path().join("STATE").join("BUTTON.JSON")), "in any case");
        assert!(file.matters(&root.path().join("state")), "its folder made or gone");
        assert!(file.matters(root.path()), "a folder further up");
        assert!(!file.matters(&root.path().join("state").join("seat.json")), "a file beside it");
        assert!(!file.matters(&root.path().join("state").join("button.json.4512.tmp")), "its writer's temporary file");
        assert!(!file.matters(&root.path().join("other")));

        let folder = Target::Folder(root.path().join("InMail"));
        assert!(folder.matters(&root.path().join("InMail").join("a.md")));
        assert!(folder.matters(&root.path().join("InMail")));
        assert!(!folder.matters(&root.path().join("InMail").join("archive").join("old.md")), "inside a folder in it");

        let mut watch = Watch::new(|_| {}).unwrap();
        watch.follow(&[file.clone(), folder.clone()]);
        let paths = [root.path().join("state").join("button.json")];
        assert!(watch.matters(&paths));
        assert_eq!(watch.which(&paths), vec![file.clone()]);
        assert!(!watch.matters(&[root.path().join("state").join("seat.json")]));
        assert!(!watch.matters(&[]));
    }

    /// Waits for a change that matters, following the targets after it, as a watching thread
    /// does: what's told before then doesn't count
    fn changed(watch: &mut Watch, rx: &Receiver<Seen>, targets: &[Target], what: &str) -> Duration {
        let start = Instant::now();
        let deadline = start + Duration::from_secs(10);
        loop {
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(Seen::Changed(paths)) if watch.matters(&paths) => break,
                Ok(Seen::Changed(_)) => continue,
                Ok(Seen::Failed(err)) => panic!("the watch failed while waiting for {what}: {err}"),
                Err(_) => panic!("no change seen in 10 s: {what}"),
            }
        }
        let took = start.elapsed();
        while watch.follow(targets) {}
        took
    }

    #[test]
    fn a_folder_on_the_way_matters_made_removed_or_renamed_not_written_in() {
        use notify::event::{CreateKind, EventKind, ModifyKind, RemoveKind, RenameMode};
        let root = tempfile::tempdir().unwrap();
        let file = Target::File(root.path().join("conductor").join("button.json"));
        let folder = root.path().join("conductor");
        let written = EventKind::Modify(ModifyKind::Any);
        assert!(file.matters_as(&folder.join("button.json"), &written), "the file written");
        assert!(!file.matters_as(&folder, &written), "a file in its folder written: its folder's time");
        assert!(file.matters_as(&folder, &EventKind::Create(CreateKind::Any)), "its folder made");
        assert!(file.matters_as(&folder, &EventKind::Remove(RemoveKind::Any)), "its folder removed");
        assert!(file.matters_as(&folder, &EventKind::Modify(ModifyKind::Name(RenameMode::To))), "its folder renamed");
        let inbox = Target::Folder(root.path().join("P").join("InMail"));
        assert!(inbox.matters_as(&root.path().join("P").join("InMail").join("a.md"), &written));
        assert!(inbox.matters_as(&root.path().join("P").join("InMail"), &written), "the folder itself");
        assert!(!inbox.matters_as(&root.path().join("P"), &written), "its project's folder written in");
        assert!(inbox.matters_as(&root.path().join("P"), &EventKind::Create(CreateKind::Any)));
    }

    #[test]
    fn the_watch_sees_a_file_written_replaced_and_removed_and_not_its_neighbours() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("conductor");
        let file = dir.join("button.json");
        let targets = [Target::File(file.clone())];
        let (tx, rx) = channel();
        let mut watch = Watch::new(move |seen| {
            let _ = tx.send(seen);
        })
        .unwrap();
        assert!(watch.follow(&targets), "the first follow watches something");
        assert_eq!(watch.watched(), &[root.path().to_path_buf()], "its folder isn't there: the root is watched");
        assert!(!watch.follow(&targets), "and the next, nothing new");

        std::fs::create_dir_all(&dir).unwrap();
        changed(&mut watch, &rx, &targets, "its folder made");
        assert_eq!(watch.watched(), &[dir.clone()], "its own folder watched once it's there");
        std::thread::sleep(Duration::from_millis(100));
        while rx.try_recv().is_ok() {}

        std::fs::write(&file, "{\"asks\":0}").unwrap();
        changed(&mut watch, &rx, &targets, "the file written");
        // As its writer replaces it: a temporary file renamed over it
        let tmp = dir.join("button.json.77.tmp");
        std::fs::write(&tmp, "{\"asks\":1}").unwrap();
        std::fs::rename(&tmp, &file).unwrap();
        changed(&mut watch, &rx, &targets, "the file replaced");
        std::thread::sleep(Duration::from_millis(100));
        while rx.try_recv().is_ok() {}

        // A neighbour written and replaced, and a file in a folder beside it (which changes that
        // folder's time): Windows tells notify, which passes on nothing
        std::fs::create_dir(dir.join("estimates")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        while rx.try_recv().is_ok() {}
        std::fs::write(dir.join("seat.json.9.tmp"), "{}").unwrap();
        std::fs::rename(dir.join("seat.json.9.tmp"), dir.join("seat.json")).unwrap();
        std::fs::write(dir.join("estimates").join("e.json"), "{}").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let passed: Vec<Seen> = rx.try_iter().collect();
        assert!(passed.is_empty(), "a neighbour's change passed on: {passed:?}");

        std::fs::remove_file(&file).unwrap();
        changed(&mut watch, &rx, &targets, "the file removed");
    }

    /// Waits for what a folder going says: a change that matters or the watch's failure, either
    /// of which has whoever follows look again; then follows, as a watching thread does
    fn went(watch: &mut Watch, rx: &Receiver<Seen>, targets: &[Target], what: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(Seen::Changed(paths)) if watch.matters(&paths) => break,
                Ok(Seen::Changed(_)) => continue,
                Ok(Seen::Failed(_)) => break,
                Err(_) => panic!("nothing said in 10 s: {what}"),
            }
        }
        // What else it says comes together
        std::thread::sleep(Duration::from_millis(100));
        while rx.try_recv().is_ok() {}
        while watch.follow(targets) {}
    }

    fn watching(dir: &Path, file: &Path) -> (Watch, Receiver<Seen>, [Target; 1]) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(file, "{\"asks\":1}").unwrap();
        let targets = [Target::File(file.to_path_buf())];
        let (tx, rx) = channel();
        let mut watch = Watch::new(move |seen| {
            let _ = tx.send(seen);
        })
        .unwrap();
        while watch.follow(&targets) {}
        assert_eq!(watch.watched(), &[dir.to_path_buf()]);
        (watch, rx, targets)
    }

    #[test]
    fn a_folder_removed_and_made_again_is_watched_again() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("conductor");
        let file = dir.join("button.json");
        let (mut watch, rx, targets) = watching(&dir, &file);

        std::fs::remove_file(&file).unwrap();
        changed(&mut watch, &rx, &targets, "the file removed");
        std::thread::sleep(Duration::from_millis(100));
        while rx.try_recv().is_ok() {}
        // Windows ends the folder's watch as it goes (notify says so, where it said nothing)
        std::fs::remove_dir(&dir).unwrap();
        went(&mut watch, &rx, &targets, "its folder removed");
        assert_eq!(watch.watched(), &[root.path().to_path_buf()], "the folder above it watched once it's gone");

        std::fs::create_dir(&dir).unwrap();
        changed(&mut watch, &rx, &targets, "its folder made again");
        assert_eq!(watch.watched(), &[dir.clone()], "the new folder watched");
        std::thread::sleep(Duration::from_millis(100));
        while rx.try_recv().is_ok() {}
        std::fs::write(&file, "{\"asks\":2}").unwrap();
        changed(&mut watch, &rx, &targets, "the file written in the new folder");
    }

    #[test]
    fn a_folder_removed_and_made_again_at_once_is_watched_again() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("conductor");
        let file = dir.join("button.json");
        let (mut watch, rx, targets) = watching(&dir, &file);

        // Gone and back before anything follows: the folder watched is another than the one
        // there, though its name is the same
        std::fs::remove_file(&file).unwrap();
        std::fs::remove_dir(&dir).unwrap();
        std::fs::create_dir(&dir).unwrap();
        went(&mut watch, &rx, &targets, "its folder removed and made again");
        assert_eq!(watch.watched(), &[dir.clone()]);
        std::fs::write(&file, "{\"asks\":2}").unwrap();
        changed(&mut watch, &rx, &targets, "the file written in the new folder");
    }

    #[test]
    fn a_folder_renamed_away_and_made_again_is_watched_again() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("conductor");
        let file = dir.join("button.json");
        let (mut watch, rx, targets) = watching(&dir, &file);

        // Its own watch follows it to its new name and Windows tells it nothing: its name's
        // watch in the folder above says it went
        let away = root.path().join("conductor.old");
        std::fs::rename(&dir, &away).unwrap();
        went(&mut watch, &rx, &targets, "its folder renamed away");
        assert_eq!(watch.watched(), &[root.path().to_path_buf()], "the folder above it watched once it's gone");
        std::thread::sleep(Duration::from_millis(100));
        while rx.try_recv().is_ok() {}
        std::fs::write(away.join("button.json"), "{\"asks\":9}").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let passed: Vec<Seen> = rx.try_iter().collect();
        assert!(passed.is_empty(), "the folder renamed away is no longer watched as if it were the target's: {passed:?}");

        std::fs::create_dir(&dir).unwrap();
        changed(&mut watch, &rx, &targets, "a new folder of its name");
        assert_eq!(watch.watched(), &[dir.clone()]);
        std::thread::sleep(Duration::from_millis(100));
        while rx.try_recv().is_ok() {}
        std::fs::write(&file, "{\"asks\":3}").unwrap();
        changed(&mut watch, &rx, &targets, "the file written in the new folder");

        // And back over a folder of its name gone: renamed to it
        std::fs::remove_file(&file).unwrap();
        std::fs::remove_dir(&dir).unwrap();
        went(&mut watch, &rx, &targets, "the new folder removed");
        std::fs::rename(&away, &dir).unwrap();
        changed(&mut watch, &rx, &targets, "the old folder renamed back");
        assert_eq!(watch.watched(), &[dir.clone()]);
        std::thread::sleep(Duration::from_millis(100));
        while rx.try_recv().is_ok() {}
        std::fs::write(&file, "{\"asks\":4}").unwrap();
        changed(&mut watch, &rx, &targets, "the file written in the folder renamed back");
    }

    #[test]
    fn a_file_within_a_folder_not_there_yet_is_seen_once_the_folder_comes() {
        // Claude Code's projects folder in a config made a moment ago: the session is shown
        // before Claude Code makes the folder, its own folder in it, and the transcript. Until
        // 2026-10-31 the folder made didn't count, so nothing was watched below the one above it
        let root = tempfile::tempdir().unwrap();
        let projects = root.path().join("projects");
        let targets = [Target::Within(projects.clone(), "5d1e7f20.jsonl".into())];
        assert!(targets[0].matters(&projects), "the folder itself");
        let (tx, rx) = channel();
        let mut watch = Watch::new(move |seen| {
            let _ = tx.send(seen);
        })
        .unwrap();
        while watch.follow(&targets) {}
        assert_eq!(watch.watched(), &[root.path().to_path_buf()]);

        std::fs::create_dir(&projects).unwrap();
        changed(&mut watch, &rx, &targets, "the projects folder made");
        assert_eq!(watch.watched(), &[projects.clone()], "watched with all its folders once it's there");
        std::thread::sleep(Duration::from_millis(100));
        while rx.try_recv().is_ok() {}
        let folder = projects.join("C--work-demo");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("5d1e7f20.jsonl"), "{}\n").unwrap();
        changed(&mut watch, &rx, &targets, "the transcript written in a folder made in it");
    }

    #[test]
    fn a_folder_let_go_of_gives_way_to_the_nearest_one_above_it_that_is_there() {
        let root = tempfile::tempdir().unwrap();
        let projects = root.path().join("Projects");
        let inbox = projects.join("Resume").join("InMail");
        std::fs::create_dir_all(&inbox).unwrap();
        let let_go = [projects.join("Resume")];
        assert_eq!(outside(inbox.clone(), &let_go), Some((projects.clone(), true)), "a folder in one");
        assert_eq!(outside(let_go[0].clone(), &let_go), Some((projects.clone(), true)), "the folder itself");
        let upper = PathBuf::from(inbox.to_string_lossy().to_uppercase());
        assert_eq!(outside(upper, &let_go), Some((projects.clone(), true)), "in any case");
        assert_eq!(outside(projects.join("Career"), &let_go), Some((projects.join("Career"), false)), "a neighbour");
        let nested = [projects.join("Resume"), projects.clone()];
        assert_eq!(outside(inbox.clone(), &nested), Some((root.path().to_path_buf(), true)), "past every one it's in");
        let gone = [root.path().join("Gone").join("Deeper")];
        assert_eq!(
            outside(gone[0].join("InMail"), &gone),
            Some((root.path().to_path_buf(), true)),
            "the nearest above that's there"
        );
        assert_eq!(outside(inbox.clone(), &[]), Some((inbox, false)));
    }

    /// The folders let go of are the process's: the tests that set them take turns
    static LETTING_GO: Mutex<()> = Mutex::new(());

    #[test]
    fn a_folder_let_go_of_can_be_renamed_and_what_is_in_it_is_watched_again_once_taken_back() {
        let _turn = locked(&LETTING_GO);
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("Resume");
        let inbox = project.join("InMail");
        std::fs::create_dir_all(&inbox).unwrap();
        let targets = [Target::Folder(inbox.clone())];
        let (tx, rx) = channel();
        let mut watch = Watch::new(move |seen| {
            let _ = tx.send(seen);
        })
        .unwrap();
        while watch.follow(&targets) {}
        assert_eq!(watch.watched(), &[inbox.clone()]);
        // What 2026-11-03's rename of a project met: a handle inside the folder (the InMail
        // folder's watch, and its name's in the project folder) and Windows refuses it
        let career = root.path().join("Career");
        assert!(std::fs::rename(&project, &career).is_err(), "renamed with a watch inside it");

        assert!(let_go(vec![project.clone(), PathBuf::from(project.to_string_lossy().to_uppercase())]));
        assert_eq!(let_go_of().len(), 1, "a folder named twice is let go of once");
        assert!(!let_go(vec![project.clone()]), "the same set again changes nothing");
        // Told at once, as for a folder on the way to its target, and the watch steps out
        changed(&mut watch, &rx, &targets, "the project let go of");
        assert_eq!(watch.watched(), &[root.path().to_path_buf()], "the folder above it, nothing inside it");
        std::fs::rename(&project, &career).expect("renamed once it's let go of");
        // The rename seen from above, as any folder on the way to a target
        changed(&mut watch, &rx, &targets, "the project renamed away");
        std::thread::sleep(Duration::from_millis(100));
        while rx.try_recv().is_ok() {}

        // Taken back while its folder isn't there: still watched from above, and its new folder
        // watched itself once it comes
        assert!(let_go(vec![]));
        changed(&mut watch, &rx, &targets, "the project taken back");
        assert_eq!(watch.watched(), &[root.path().to_path_buf()]);
        std::fs::create_dir_all(&inbox).unwrap();
        changed(&mut watch, &rx, &targets, "a project of that name made again, with its InMail");
        assert_eq!(watch.watched(), &[inbox.clone()]);
        std::thread::sleep(Duration::from_millis(100));
        while rx.try_recv().is_ok() {}
        std::fs::write(inbox.join("2026-11-03-2300-from-Root-hi.md"), "hi").unwrap();
        changed(&mut watch, &rx, &targets, "a message in the folder watched again");
    }

    #[test]
    fn a_folder_let_go_of_tells_only_the_watches_it_is_about() {
        let _turn = locked(&LETTING_GO);
        let root = tempfile::tempdir().unwrap();
        let one = root.path().join("One");
        let two = root.path().join("Two");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        let (tx, rx) = channel();
        let mut other = Watch::new(move |seen| {
            let _ = tx.send(seen);
        })
        .unwrap();
        while other.follow(&[Target::File(two.join("button.json"))]) {}
        std::thread::sleep(Duration::from_millis(100));
        while rx.try_recv().is_ok() {}
        assert!(let_go(vec![one.clone()]));
        assert!(let_go(vec![]));
        std::thread::sleep(Duration::from_millis(100));
        let passed: Vec<Seen> = rx.try_iter().collect();
        assert!(passed.is_empty(), "a watch with nothing in the folder let go of was told: {passed:?}");
        assert_eq!(other.watched(), &[two.clone()]);
    }

    #[test]
    fn more_changed_at_once_than_windows_holds_is_a_failure_and_the_watch_goes_on() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("InMail");
        std::fs::create_dir(&dir).unwrap();
        let targets = [Target::Folder(dir.clone())];
        let (tx, rx) = channel();
        let (go_tx, go_rx) = channel::<()>();
        let mut first = true;
        // The first change holds notify's thread until the test lets it go, as a slow handler
        // would: what changes meanwhile Windows keeps for the watch's next read, up to what that
        // holds, and keeps none of it past that
        let mut watch = Watch::new(move |seen| {
            if std::mem::take(&mut first) {
                let _ = go_rx.recv_timeout(Duration::from_secs(10));
            }
            let _ = tx.send(seen);
        })
        .unwrap();
        while watch.follow(&targets) {}
        std::fs::write(dir.join("first.md"), "first").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let name = "a-message-with-a-name-as-long-as-inmail-gives-one-from-a-project";
        for i in 0..1500 {
            std::fs::write(dir.join(format!("{name}-{i:04}.md")), "x").unwrap();
        }
        go_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let failed = loop {
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(Seen::Failed(err)) => break err,
                Ok(Seen::Changed(_)) => continue,
                Err(_) => panic!("1500 files written while the watch's thread was held said no failure"),
            }
        };
        assert!(failed.contains("more changed at once"), "{failed}");
        std::thread::sleep(Duration::from_millis(100));
        while rx.try_recv().is_ok() {}
        while watch.follow(&targets) {}
        assert_eq!(watch.watched(), &[dir.clone()]);
        std::fs::write(dir.join("last.md"), "last").unwrap();
        changed(&mut watch, &rx, &targets, "a file written after");
    }
}
